//! The only raw JVM boundary. Swing and policy code use safe JNI wrappers.
use crate::swing::Automation;
use ibkit_gateway_core::{AgentLaunch, ErrorCode, EventKind};
use jni::{
    JNIEnv, JavaVM, NativeMethod,
    objects::{GlobalRef, JObject, JValue},
    sys::{JNI_ERR, JNI_OK, JavaVM as RawVm, jint},
};
use jvmti_sys::{
    JVMTI_VERSION_1_2, jthread, jvmtiEnv, jvmtiEvent, jvmtiEventCallbacks, jvmtiEventMode,
};
use std::{
    ffi::{CStr, c_char, c_void},
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread,
    time::Duration,
};

static LAUNCH: OnceLock<AgentLaunch> = OnceLock::new();
static AUTOMATION: OnceLock<Mutex<Automation>> = OnceLock::new();
static ALIVE: AtomicBool = AtomicBool::new(true);
static PENDING: AtomicBool = AtomicBool::new(false);
/// The Dispatch instance: the Runnable queued on the EDT and the window
/// event listener.
static DISPATCH: OnceLock<GlobalRef> = OnceLock::new();
/// Passes queued at once after an action, so multi-step flows do not wait
/// for the periodic timer; bounded so a repeating action cannot spin.
static CHAIN: AtomicU8 = AtomicU8::new(0);
const MAX_CHAIN: u8 = 8;
/// AWT WindowEvent IDs that can change what the agent must do.
const WINDOW_OPENED: i32 = 200;
const WINDOW_CLOSED: i32 = 202;
const WINDOW_ACTIVATED: i32 = 205;

/// Queue one automation pass on the EDT unless one is already pending.
fn queue_pass(env: &mut JNIEnv<'_>) -> jni::errors::Result<()> {
    if let Some(dispatch) = DISPATCH.get()
        && !PENDING.swap(true, Ordering::AcqRel)
    {
        env.call_static_method(
            "javax/swing/SwingUtilities",
            "invokeLater",
            "(Ljava/lang/Runnable;)V",
            &[JValue::Object(dispatch.as_obj())],
        )?;
    }
    Ok(())
}

/// # Safety
/// The JVM calls this symbol with its live VM pointer and a NUL-terminated
/// option string. No caller may supply arbitrary pointers.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Agent_OnLoad(
    vm: *mut RawVm,
    options: *mut c_char,
    _: *mut c_void,
) -> jint {
    // The reason goes to the launcher's output (state_dir/launcher-stdio.log),
    // where the controller reads it.
    let fail = |reason: String| {
        eprintln!("ibkit agent: {reason}");
        JNI_ERR
    };
    catch_unwind(AssertUnwindSafe(|| {
        if vm.is_null() || options.is_null() {
            return fail("the JVM passed no options".into());
        }
        // SAFETY: Agent_OnLoad's JVM ABI supplies a live terminated option string.
        let Ok(path) = (unsafe { CStr::from_ptr(options) }).to_str() else {
            return fail("the agent options are not UTF-8".into());
        };
        let config_path = if path == "env" {
            std::env::var("IBKIT_AGENT_CONFIG").unwrap_or_default()
        } else {
            path.to_owned()
        };
        let bytes = match fs::read(&config_path) {
            Ok(bytes) => bytes,
            Err(error) => return fail(format!("cannot read the launch file {config_path:?}: {error}")),
        };
        let launch = match serde_json::from_slice::<AgentLaunch>(&bytes) {
            Ok(launch) => launch,
            Err(error) => {
                // serde lists every expected field; the unknown one is enough.
                let error = error.to_string();
                let error = error.split(", expected").next().unwrap_or(&error);
                return fail(format!(
                    "the launch file {config_path} does not match this agent ({error}); the agent is from another ibkit build"
                ));
            }
        };
        if let Err(error) = launch.config.validate() {
            return fail(format!("the launch settings are invalid: {error:#}"));
        }
        if LAUNCH.set(launch).is_err() {
            return fail("the agent was loaded twice".into());
        }
        let mut env: *mut c_void = ptr::null_mut();
        // SAFETY: VM and GetEnv table come from the JVM; env is an out pointer.
        let result = unsafe { (**vm).GetEnv.unwrap()(vm, &mut env, JVMTI_VERSION_1_2 as jint) };
        if result != JNI_OK || env.is_null() {
            return JNI_ERR;
        }
        let env = env.cast::<jvmtiEnv>();
        let callbacks = jvmtiEventCallbacks {
            VMInit: Some(vm_init),
            VMDeath: Some(vm_death),
            ..Default::default()
        };
        // SAFETY: GetEnv returned this JVMTI 1.2 table. The JVM copies callbacks
        // during this call. Function signatures match jvmti.h; no reference escapes.
        unsafe {
            if ((**env).SetEventCallbacks.unwrap())(
                env,
                &callbacks,
                size_of::<jvmtiEventCallbacks>() as jint,
            ) as u32
                != 0
            {
                return JNI_ERR;
            }
            for event in [
                jvmtiEvent::JVMTI_EVENT_VM_INIT,
                jvmtiEvent::JVMTI_EVENT_VM_DEATH,
            ] {
                if ((**env).SetEventNotificationMode.unwrap())(
                    env,
                    jvmtiEventMode::JVMTI_ENABLE,
                    event,
                    ptr::null_mut(),
                ) as u32
                    != 0
                {
                    return JNI_ERR;
                }
            }
        }
        JNI_OK
    }))
    .unwrap_or(JNI_ERR)
}

unsafe extern "C" fn vm_init(_: *mut jvmtiEnv, raw: *mut jni::sys::JNIEnv, _: jthread) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: VMInit supplies a JNIEnv valid for this callback thread only.
        let Ok(env) = (unsafe { JNIEnv::from_raw(raw) }) else {
            return;
        };
        let Ok(vm) = env.get_java_vm() else {
            return;
        };
        thread::spawn(move || {
            if let Err(error) = run(vm) {
                if let Some(automation) = AUTOMATION.get()
                    && let Ok(mut automation) = automation.lock()
                {
                    automation.fail(ErrorCode::JavaException);
                }
                // JNI errors carry operation names, never Java field values.
                eprintln!(
                    "Rust Gateway agent failed: {error}. Check the agent path and Java version, then start again."
                );
            }
        });
    }));
}

unsafe extern "C" fn vm_death(_: *mut jvmtiEnv, _: *mut jni::sys::JNIEnv) {
    ALIVE.store(false, Ordering::Release);
}

fn run(vm: JavaVM) -> anyhow::Result<()> {
    let launch = LAUNCH
        .get()
        .ok_or_else(|| anyhow::anyhow!("Agent configuration is absent"))?
        .clone();
    let mut env = vm.attach_current_thread_as_daemon()?;
    let restart_key = env.new_string("restart")?;
    let empty = env.new_string("")?;
    let restart = env
        .call_static_method(
            "java/lang/System",
            "getProperty",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&restart_key), JValue::Object(&empty)],
        )?
        .l()?;
    let restarting = !env
        .get_string(&jni::objects::JString::from(restart))?
        .is_empty();
    let automation = match Automation::new(launch.clone(), restarting) {
        Ok(automation) => automation,
        Err(error) => {
            let _ = crate::swing::report_start_failure(&launch, &error);
            return Err(error);
        }
    };
    AUTOMATION
        .set(Mutex::new(automation))
        .map_err(|_| anyhow::anyhow!("Agent is already running"))?;
    let loader = env
        .call_static_method(
            "java/lang/ClassLoader",
            "getSystemClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )?
        .l()?;
    // JNI-attached threads have the bootstrap context loader by default.
    // AWT can inherit it when it starts its event thread. Gateway's custom UI
    // classes must remain visible through the application class loader.
    let thread = env
        .call_static_method(
            "java/lang/Thread",
            "currentThread",
            "()Ljava/lang/Thread;",
            &[],
        )?
        .l()?;
    env.call_method(
        &thread,
        "setContextClassLoader",
        "(Ljava/lang/ClassLoader;)V",
        &[JValue::Object(&loader)],
    )?;
    let class = env.define_class(
        "org/ibkit/Dispatch",
        &loader,
        include_bytes!(concat!(env!("OUT_DIR"), "/Dispatch.class")),
    )?;
    env.register_native_methods(
        &class,
        &[
            NativeMethod {
                name: "run".into(),
                sig: "()V".into(),
                fn_ptr: dispatch as *mut c_void,
            },
            NativeMethod {
                name: "eventDispatched".into(),
                sig: "(Ljava/awt/AWTEvent;)V".into(),
                fn_ptr: window_event as *mut c_void,
            },
        ],
    )?;
    let runnable = env.new_object(&class, "()V", &[])?;
    let _ = DISPATCH.set(env.new_global_ref(&runnable)?);
    if let Some(automation) = AUTOMATION.get() {
        automation.lock().unwrap().emit(EventKind::Started)?;
    }
    // Do not start Swing's event thread before Gateway installs its look and
    // feel. Window.getWindows is synchronized and creates no UI components.
    // This also prevents an agent from opening an otherwise absent GUI.
    while ALIVE.load(Ordering::Acquire) {
        let windows = env
            .call_static_method("java/awt/Window", "getWindows", "()[Ljava/awt/Window;", &[])?
            .l()?;
        let windows = jni::objects::JObjectArray::from(windows);
        let count = env.get_array_length(&windows)?;
        env.delete_local_ref(windows)?;
        if count != 0 {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    // React to window events at once; the timer below still drives
    // heartbeats, commands and time limits.
    let toolkit = env
        .call_static_method(
            "java/awt/Toolkit",
            "getDefaultToolkit",
            "()Ljava/awt/Toolkit;",
            &[],
        )?
        .l()?;
    env.call_method(
        &toolkit,
        "addAWTEventListener",
        "(Ljava/awt/event/AWTEventListener;J)V",
        &[JValue::Object(&runnable), JValue::Long(0x40)],
    )?;
    while ALIVE.load(Ordering::Acquire) {
        queue_pass(&mut env)?;
        thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

/// AWTEventListener.eventDispatched, called on the EDT for window events.
extern "system" fn window_event(mut env: JNIEnv<'_>, _: JObject<'_>, event: JObject<'_>) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let id = env
            .call_method(&event, "getID", "()I", &[])
            .and_then(|value| value.i());
        if matches!(id, Ok(WINDOW_OPENED | WINDOW_CLOSED | WINDOW_ACTIVATED)) {
            let _ = queue_pass(&mut env);
        }
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
    }));
}

extern "system" fn dispatch(mut env: JNIEnv<'_>, _: JObject<'_>) {
    PENDING.store(false, Ordering::Release);
    let mut acted = false;
    let outcome = catch_unwind(AssertUnwindSafe(|| -> anyhow::Result<()> {
        let Some(automation) = AUTOMATION.get() else {
            return Ok(());
        };
        // Each callback gets a new local frame, including on a long-running EDT.
        env.with_local_frame(8192, |env| {
            let action = match automation.try_lock() {
                Ok(mut automation) => match automation.tick(env) {
                    Ok(action) => {
                        automation.tick_succeeded();
                        action
                    }
                    Err(error) => {
                        if env.exception_check().unwrap_or(false) {
                            let _ = env.exception_clear();
                        }
                        automation.tick_failed(&error);
                        None
                    }
                },
                // A nested event loop inside the locked pass (a modal dialog
                // that a setter opened): answer known confirmations so the
                // outer pass can return; waiting for the lock would deadlock.
                Err(std::sync::TryLockError::WouldBlock) => {
                    let config = &LAUNCH
                        .get()
                        .ok_or(jni::errors::Error::NullPtr("agent configuration"))?
                        .config;
                    match crate::swing::answer_nested_confirmation(env, config) {
                        Ok(action) => action,
                        Err(_) => {
                            if env.exception_check().unwrap_or(false) {
                                let _ = env.exception_clear();
                            }
                            None
                        }
                    }
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(jni::errors::Error::NullPtr("agent state"));
                }
            };
            // Button listeners can open modal dialogs and enter a nested event
            // loop. Never hold the Rust state lock across their invocation.
            if let Some(action) = action {
                action.run(env)?;
                acted = true;
            }
            Ok::<_, jni::errors::Error>(())
        })?;
        Ok(())
    }));
    // Run the next step at once after an action (bounded); the queued pass
    // runs after the events the action posted.
    if acted && CHAIN.fetch_add(1, Ordering::AcqRel) < MAX_CHAIN {
        let _ = queue_pass(&mut env);
    } else if !acted {
        CHAIN.store(0, Ordering::Release);
    }
    if !matches!(outcome, Ok(Ok(()))) {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        if let Some(automation) = AUTOMATION.get()
            && let Ok(mut automation) = automation.lock()
        {
            automation.fail(ErrorCode::JavaException);
        }
    }
}
