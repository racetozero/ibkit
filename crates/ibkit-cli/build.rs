use std::{env, fs, path::PathBuf};

/// Minimal class-file writer for the one generated class.
struct Pool(Vec<u8>, u16);

impl Pool {
    fn utf8(&mut self, value: &str) -> u16 {
        self.0.push(1);
        self.0
            .extend_from_slice(&(value.len() as u16).to_be_bytes());
        self.0.extend_from_slice(value.as_bytes());
        self.next()
    }
    fn class(&mut self, name: &str) -> u16 {
        let name = self.utf8(name);
        self.0.push(7);
        self.0.extend_from_slice(&name.to_be_bytes());
        self.next()
    }
    fn method_ref(&mut self, class: u16, name: &str, descriptor: &str) -> u16 {
        let (name, descriptor) = (self.utf8(name), self.utf8(descriptor));
        self.0.push(12);
        self.0.extend_from_slice(&name.to_be_bytes());
        self.0.extend_from_slice(&descriptor.to_be_bytes());
        let pair = self.next();
        self.0.push(10);
        self.0.extend_from_slice(&class.to_be_bytes());
        self.0.extend_from_slice(&pair.to_be_bytes());
        self.next()
    }
    fn next(&mut self) -> u16 {
        self.1 += 1;
        self.1
    }
}

fn main() {
    // Class-file equivalent of:
    // public final class Dispatch implements Runnable, java.awt.event.AWTEventListener {
    //   public Dispatch() { super(); }
    //   public native void run();
    //   public native void eventDispatched(java.awt.AWTEvent event);
    // }
    // No Java compiler, JAR, or Java automation code is needed.
    let mut pool = Pool(Vec::new(), 0);
    let this = pool.class("org/ibkit/Dispatch");
    let object = pool.class("java/lang/Object");
    let runnable = pool.class("java/lang/Runnable");
    let listener = pool.class("java/awt/event/AWTEventListener");
    let super_init = pool.method_ref(object, "<init>", "()V");
    let init = pool.utf8("<init>");
    let void = pool.utf8("()V");
    let code = pool.utf8("Code");
    let run = pool.utf8("run");
    let dispatched = pool.utf8("eventDispatched");
    let event = pool.utf8("(Ljava/awt/AWTEvent;)V");

    let mut bytes = Vec::new();
    let u16s = |bytes: &mut Vec<u8>, values: &[u16]| {
        for value in values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
    };
    bytes.extend_from_slice(&0xcafebabe_u32.to_be_bytes());
    u16s(&mut bytes, &[0, 52, pool.1 + 1]);
    bytes.extend_from_slice(&pool.0);
    // public final super; this, super, two interfaces; no fields.
    u16s(
        &mut bytes,
        &[0x31, this, object, 2, runnable, listener, 0, 3],
    );
    // public <init>: aload_0; invokespecial Object.<init>; return.
    u16s(&mut bytes, &[1, init, void, 1, code]);
    bytes.extend_from_slice(&17_u32.to_be_bytes());
    u16s(&mut bytes, &[1, 1]);
    bytes.extend_from_slice(&5_u32.to_be_bytes());
    bytes.extend_from_slice(&[0x2a, 0xb7]);
    bytes.extend_from_slice(&super_init.to_be_bytes());
    bytes.push(0xb1);
    u16s(&mut bytes, &[0, 0]);
    // public native methods have no Code attribute.
    u16s(&mut bytes, &[0x101, run, void, 0]);
    u16s(&mut bytes, &[0x101, dispatched, event, 0]);
    // No class attributes.
    u16s(&mut bytes, &[0]);
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("Dispatch.class"),
        bytes,
    )
    .unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
