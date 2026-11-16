import java.awt.*;
import java.awt.event.*;
import javax.swing.*;
import javax.swing.tree.*;

/** An independent Swing application. All assertions inspect UI results. */
public final class GatewayFixture {
    static JFrame main;
    static int configured;
    static boolean warningAccepted;
    static boolean logExported;
    static int logExportCount;
    static boolean reconnected;
    static final int port = Integer.parseInt(System.getenv("FIXTURE_PORT"));

    static void require(boolean value, String message) {
        if (!SwingUtilities.isEventDispatchThread()) throw new AssertionError("The UI operation used the wrong thread");
        if (Thread.currentThread().getContextClassLoader() != GatewayFixture.class.getClassLoader()) throw new AssertionError("The UI thread lost the application class loader");
        if (!value) { System.err.println(message); System.exit(2); }
    }

    public static void main(String[] args) throws Exception {
        // Give the native agent the first opportunity to start Swing's event
        // thread, as can happen with an install4j splash window.
        Window probe = new Window((Window)null);
        Thread.sleep(1200);
        SwingUtilities.invokeLater(GatewayFixture::login);
        // A hung agent must fail the test without an unbounded wait.
        Thread watchdog = new Thread(() -> { try { Thread.sleep(120000); } catch (InterruptedException ignored) {} System.exit(3); });
        watchdog.setDaemon(true); watchdog.start();
    }

    static void login() {
        main = new JFrame("IBKR Gateway");
        JPanel content = new JPanel(new GridLayout(0, 1));
        JToggleButton api = new JToggleButton("IB API", true);
        JToggleButton live = new JToggleButton("Live Trading", true);
        JToggleButton paper = new JToggleButton("Paper Trading", false);
        ButtonGroup group = new ButtonGroup(); group.add(live); group.add(paper);
        JTextField username = new JTextField();
        JPasswordField password = new JPasswordField();
        JButton submit = new JButton("Paper Log In");
        // Like Gateway: Log In stays disabled until both fields have text.
        submit.setEnabled(false);
        Runnable enable = () -> submit.setEnabled(!username.getText().isEmpty() && password.getPassword().length > 0);
        username.getDocument().addDocumentListener(onChange(enable));
        password.getDocument().addDocumentListener(onChange(enable));
        submit.addActionListener(event -> {
            require(!"restore".equals(System.getenv("FIXTURE_SCENARIO")), "Restart restoration must not submit credentials");
            require(paper.isSelected(), "Paper trading was not selected");
            require(username.getText().equals("fixture-user"), "Username was not entered");
            require(new String(password.getPassword()).equals("fixture-secret"), "Password was not entered");
            main.dispose();
            if ("progress".equals(System.getenv("FIXTURE_SCENARIO"))) {
                // Gateway's login progress modal: OK cancels the login.
                JDialog progress = new JDialog((Frame)null, "Gateway", true);
                JButton cancelLogin = new JButton("OK");
                cancelLogin.addActionListener(e -> { System.err.println("The progress dialog was clicked"); System.exit(5); });
                progress.add(new JLabel("Attempt 1: connecting to server (trying for another 20 seconds)"));
                progress.add(cancelLogin, BorderLayout.SOUTH); progress.pack();
                Timer done = new Timer(2500, e -> progress.dispose()); done.setRepeats(false); done.start();
                progress.setVisible(true);
                secondFactor();
            } else if ("unknown".equals(System.getenv("FIXTURE_SCENARIO"))) {
                JDialog unknown = new JDialog((Frame)null, "Unexpected margin notice", true);
                unknown.add(new JTextField("fixture-secret"));
                unknown.add(new JLabel("Human review required"), BorderLayout.NORTH);
                unknown.add(new JButton("OK"), BorderLayout.SOUTH);
                unknown.pack(); unknown.setVisible(true);
            } else if ("account_tasks".equals(System.getenv("FIXTURE_SCENARIO"))) {
                JOptionPane.showMessageDialog(null, "Complete account tasks", "Login Messages", JOptionPane.INFORMATION_MESSAGE);
            } else if ("security".equals(System.getenv("FIXTURE_SCENARIO"))) {
                JOptionPane.showMessageDialog(null, "Enter card code", "Security Code Card Authentication", JOptionPane.INFORMATION_MESSAGE);
            } else if ("existing".equals(System.getenv("FIXTURE_SCENARIO"))) {
                JOptionPane.showMessageDialog(null, "A second session is active", "Existing session detected", JOptionPane.INFORMATION_MESSAGE);
            } else if ("pwnotice".equals(System.getenv("FIXTURE_SCENARIO"))) {
                // Gateway 10.52 Password_to_expire, in a JFrame as IBC expects.
                JFrame notice = new JFrame("Password Notice");
                JEditorPane pane = new JEditorPane("text/html", "<html>Your password will expire in <b>5</b> day(s).<br><br>You may change your password at any time by proceeding to <a href='#change_pass'>Account Management</a>.</html>");
                JButton ok = new JButton("OK"); ok.addActionListener(e -> notice.dispose());
                notice.add(pane); notice.add(ok, BorderLayout.SOUTH); notice.pack(); notice.setVisible(true);
                mainWindow();
            } else if ("totp_wrong".equals(System.getenv("FIXTURE_SCENARIO"))) {
                // A code prompt that never accepts a code.
                JDialog code = new JDialog((Frame)null, "Second Factor Authentication", true);
                JTextField field = new JTextField(8); JButton ok = new JButton("OK"); JButton cancel = new JButton("Cancel");
                cancel.addActionListener(e -> code.dispose());
                JPanel buttons = new JPanel(); buttons.add(ok); buttons.add(cancel);
                code.add(new JLabel("Enter Mobile Authenticator app code"), BorderLayout.NORTH); code.add(field); code.add(buttons, BorderLayout.SOUTH);
                code.pack(); code.setVisible(true);
            } else if ("early_main".equals(System.getenv("FIXTURE_SCENARIO"))) {
                // Gateway 10.50: the main window opens before the second
                // factor, with Settings disabled until the login completes.
                mainWindow();
            } else secondFactor();
        });
        JCheckBox ssl = new JCheckBox("Use SSL");
        // A listener that opens a modal dialog while the agent sets the box:
        // the agent's pass is still running inside the nested event loop.
        if ("reentrant".equals(System.getenv("FIXTURE_SCENARIO"))) ssl.addItemListener(e -> {
            JDialog note = new JOptionPane("SSL will be used for this connection", JOptionPane.INFORMATION_MESSAGE).createDialog(main, "Note");
            Timer close = new Timer(3000, event -> note.dispose()); close.setRepeats(false); close.start();
            note.setVisible(true);
        });
        if ("reentrant_known".equals(System.getenv("FIXTURE_SCENARIO"))) ssl.addItemListener(e -> {
            JDialog confirm = new JDialog(main, "IBKR Gateway", true);
            JButton yes = new JButton("Yes"); yes.addActionListener(event -> confirm.dispose());
            confirm.add(new JLabel("Do you want to apply the new socket port setting?"), BorderLayout.NORTH);
            confirm.add(yes, BorderLayout.SOUTH); confirm.pack(); confirm.setVisible(true);
        });
        for (Component component : new Component[] {api, live, paper, username, password, ssl, submit}) content.add(component);
        main.setContentPane(content); main.pack(); main.setVisible(true);
        if ("restore".equals(System.getenv("FIXTURE_SCENARIO"))) {
            Timer restore = new Timer(2000, event -> { main.dispose(); mainWindow(); }); restore.setRepeats(false); restore.start();
        }
    }

    static javax.swing.event.DocumentListener onChange(Runnable action) {
        return new javax.swing.event.DocumentListener() {
            public void insertUpdate(javax.swing.event.DocumentEvent e) { action.run(); }
            public void removeUpdate(javax.swing.event.DocumentEvent e) { action.run(); }
            public void changedUpdate(javax.swing.event.DocumentEvent e) { action.run(); }
        };
    }

    static String totp(long step) throws Exception {
        javax.crypto.Mac mac = javax.crypto.Mac.getInstance("HmacSHA1");
        mac.init(new javax.crypto.spec.SecretKeySpec("12345678901234567890".getBytes("US-ASCII"), "HmacSHA1"));
        byte[] h = mac.doFinal(java.nio.ByteBuffer.allocate(8).putLong(step).array());
        int o = h[19] & 0xf;
        int v = ((h[o] & 0x7f) << 24) | ((h[o + 1] & 0xff) << 16) | ((h[o + 2] & 0xff) << 8) | (h[o + 3] & 0xff);
        return String.format("%06d", v % 1000000);
    }

    static void mobileAuthenticator() {
        JDialog selection = new JDialog((Frame)null, "Second Factor Authentication", true);
        JList<String> methods = new JList<>(new String[]{"IB Key", "Mobile Authenticator app"});
        methods.setSelectedIndex(0);
        JButton ok = new JButton("OK");
        ok.addActionListener(event -> { require("Mobile Authenticator app".equals(methods.getSelectedValue()), "Mobile Authenticator was not selected"); selection.dispose(); });
        JPanel panel = new JPanel(new BorderLayout()); panel.add(new JTextArea("Select second factor device"), BorderLayout.NORTH); panel.add(methods); panel.add(ok, BorderLayout.SOUTH);
        selection.setContentPane(panel); selection.pack(); selection.setVisible(true);
        JDialog code = new JDialog((Frame)null, "Second Factor Authentication", true);
        JTextField field = new JTextField(8);
        JButton submit = new JButton("OK");
        // Like Gateway: OK stays disabled until the field has a code.
        submit.setEnabled(false);
        field.getDocument().addDocumentListener(onChange(() -> submit.setEnabled(!field.getText().isEmpty())));
        submit.addActionListener(event -> {
            long step = System.currentTimeMillis() / 30000;
            try {
                String text = field.getText();
                require(text.equals(totp(step)) || text.equals(totp(step - 1)), "The TOTP code was wrong");
            } catch (Exception e) { throw new RuntimeException(e); }
            code.dispose();
        });
        code.add(new JLabel("Enter Mobile Authenticator app code"), BorderLayout.NORTH); code.add(field); code.add(submit, BorderLayout.SOUTH);
        code.pack(); code.setVisible(true);
        mainWindow();
    }

    static void secondFactor() {
        if ("totp".equals(System.getenv("FIXTURE_SCENARIO"))) { mobileAuthenticator(); return; }
        JDialog selection = new JDialog((Frame)null, "Second Factor Authentication", true);
        JList<String> methods = new JList<>(new String[]{"Code Card", "IB Key"});
        JButton ok = new JButton("OK");
        ok.addActionListener(event -> { require("IB Key".equals(methods.getSelectedValue()), "IB Key was not selected"); selection.dispose(); });
        JPanel panel = new JPanel(new BorderLayout()); panel.add(new JTextArea("Select second factor device"), BorderLayout.NORTH); panel.add(methods); panel.add(ok, BorderLayout.SOUTH);
        selection.setContentPane(panel); selection.pack(); selection.setVisible(true);
        JDialog approval = new JDialog((Frame)null, "Second Factor Authentication", true);
        JButton cancel = new JButton("Cancel"); cancel.addActionListener(event -> { System.exit("timeout".equals(System.getenv("FIXTURE_SCENARIO")) ? 0 : 4); });
        approval.add(cancel); approval.pack();
        if (!"timeout".equals(System.getenv("FIXTURE_SCENARIO"))) {
            Timer timer = new Timer(1500, event -> approval.dispose()); timer.setRepeats(false); timer.start();
        }
        approval.setVisible(true);
        if ("reopen".equals(System.getenv("FIXTURE_SCENARIO"))) {
            JDialog reopened = new JDialog((Frame)null, "Second Factor Authentication", true);
            JButton stop = new JButton("Cancel"); stop.addActionListener(e -> reopened.dispose()); reopened.add(stop); reopened.pack(); reopened.setVisible(true);
            return;
        }
        mainWindow();
    }

    static void mainWindow() {
        main = new JFrame("IBKR Gateway");
        main.setDefaultCloseOperation(JFrame.DO_NOTHING_ON_CLOSE);
        main.addWindowListener(new WindowAdapter() {
            public void windowClosing(WindowEvent event) {
                int answer = JOptionPane.showConfirmDialog(main, "Are you sure you want to exit?", "Exit", JOptionPane.YES_NO_OPTION);
                require(answer == JOptionPane.YES_OPTION, "Shutdown was not confirmed");
                require(configured >= 2 && warningAccepted && logExported && logExportCount >= 2 && reconnected, "The fixture did not complete configuration, warning, export, reconnect, and restart checks");
                System.out.println("FIXTURE_COMPLETE");
                main.dispose(); System.exit(0);
            }
        });
        main.addKeyListener(new KeyAdapter() {
            public void keyPressed(KeyEvent event) {
                int shortcut = Toolkit.getDefaultToolkit().getMenuShortcutKeyMaskEx() | InputEvent.ALT_DOWN_MASK;
                if (event.getKeyCode() == KeyEvent.VK_R && (event.getModifiersEx() & shortcut) == shortcut) reconnected = true;
            }
        });
        JMenuBar bar = new JMenuBar(); JMenu file = new JMenu("File"); JMenu configure = new JMenu("Configure");
        JMenuItem close = new JMenuItem("Close"); close.addActionListener(event -> main.dispatchEvent(new WindowEvent(main, WindowEvent.WINDOW_CLOSING)));
        JMenuItem logs = new JMenuItem("Gateway Logs"); logs.addActionListener(event -> logs());
        JMenuItem apiLogs = new JMenuItem("API Logs"); apiLogs.addActionListener(event -> logs());
        JMenuItem settings = new JMenuItem("Settings"); settings.addActionListener(event -> configuration());
        file.add(close); file.add(logs); file.add(apiLogs); configure.add(settings); bar.add(file); bar.add(configure);
        main.setJMenuBar(bar); main.add(new JLabel("Connected")); main.pack(); main.setVisible(true);
        String scenario = System.getenv("FIXTURE_SCENARIO");
        if (configured == 0 && "early_main".equals(scenario)) {
            settings.setEnabled(false);
            Timer factor = new Timer(1500, event -> {
                JDialog approval = new JDialog(main, "Second Factor Authentication", true);
                JButton cancel = new JButton("Cancel"); cancel.addActionListener(e -> System.exit(4));
                approval.add(cancel); approval.pack();
                Timer approved = new Timer(1500, e -> {
                    approval.dispose();
                    Timer enable = new Timer(1000, done -> settings.setEnabled(true)); enable.setRepeats(false); enable.start();
                }); approved.setRepeats(false); approved.start();
                approval.setVisible(true);
            }); factor.setRepeats(false); factor.start();
        }
        if (configured == 0 && ("factor_stall".equals(scenario) || "factor_reauth".equals(scenario) || "login_reauth".equals(scenario))) {
            Timer reauth = new Timer(100, event -> {
                if (!new java.io.File(System.getenv("FIXTURE_REAUTH_REQUEST")).isFile()) return;
                ((Timer)event.getSource()).stop();
                require(configured == 1, "Initial configuration did not finish before re-authentication");
                if ("login_reauth".equals(scenario)) {
                    main.dispose(); login();
                    return;
                }
                JDialog approval = new JDialog(main, "Second Factor Authentication", true);
                JButton cancel = new JButton("Cancel"); cancel.addActionListener(e -> approval.dispose());
                approval.add(cancel); approval.pack();
                Timer approved = new Timer(1500, e -> {
                    if ("factor_stall".equals(scenario)) {
                        main.setJMenuBar(null); main.setContentPane(new JLabel("Waiting for authentication")); main.pack();
                    }
                    approval.dispose();
                }); approved.setRepeats(false); approved.start(); approval.setVisible(true);
            }); reauth.start();
        }
        if ("late_button".equals(System.getenv("FIXTURE_SCENARIO"))) {
            // A dialog whose button is disabled for 2 s, as while Gateway redraws.
            JDialog tip = new JDialog(main, "Tip of the Day", false);
            JButton tipClose = new JButton("Close"); tipClose.setEnabled(false);
            tipClose.addActionListener(e -> {
                tip.dispose();
                try { java.nio.file.Files.write(java.nio.file.Paths.get(System.getenv("FIXTURE_REAUTH_REQUEST") + ".done"), new byte[0]); } catch (Exception ignored) {}
            });
            tip.add(tipClose); tip.pack(); tip.setVisible(true);
            Timer enable = new Timer(2000, e -> tipClose.setEnabled(true)); enable.setRepeats(false); enable.start();
        }
        if ("existing2".equals(System.getenv("FIXTURE_SCENARIO"))) {
            // Two takeovers in one Ready session.
            Timer conflicts = new Timer(200, null);
            conflicts.addActionListener(event -> {
                if (!new java.io.File(System.getenv("FIXTURE_REAUTH_REQUEST")).isFile()) return;
                conflicts.stop();
                for (int i = 1; i <= 2; i++) {
                    JDialog existing = new JDialog(main, "Existing session detected", true);
                    JButton proceed = new JButton("Continue Login"); proceed.addActionListener(e -> existing.dispose());
                    JPanel buttons = new JPanel(); buttons.add(proceed); buttons.add(new JButton("Cancel"));
                    existing.add(buttons); existing.pack(); existing.setVisible(true);
                }
                try { java.nio.file.Files.write(java.nio.file.Paths.get(System.getenv("FIXTURE_REAUTH_REQUEST") + ".done"), new byte[0]); } catch (Exception ignored) {}
            });
            conflicts.start();
        }
        if ("expired".equals(System.getenv("FIXTURE_SCENARIO"))) {
            Timer expired = new Timer(2500, event -> JOptionPane.showMessageDialog(main, "Soft token=0 received instead of expected permanent")); expired.setRepeats(false); expired.start();
        }
        if ("relogin_stall".equals(System.getenv("FIXTURE_SCENARIO"))) {
            Timer relogin = new Timer(2500, event -> {
                JDialog dialog = new JDialog(main, "Re-login is required", true);
                JButton button = new JButton("Re-login"); button.addActionListener(e -> { dialog.dispose(); main.setJMenuBar(null); main.setContentPane(new JLabel("Waiting for authentication")); main.pack(); });
                dialog.add(button); dialog.pack(); dialog.setVisible(true);
            }); relogin.setRepeats(false); relogin.start();
        }
    }

    static void configuration() {
        for (Window window : Window.getWindows()) {
            if (window instanceof Dialog && "Second Factor Authentication".equals(((Dialog)window).getTitle())) {
                require(!window.isShowing(), "Settings opened before authentication finished");
            }
        }
        JDialog dialog = new JDialog(main, "IBKR Gateway Configuration", true);
        DefaultMutableTreeNode root = new DefaultMutableTreeNode("Configuration");
        DefaultMutableTreeNode api = new DefaultMutableTreeNode("API"); root.add(api);
        api.add(new DefaultMutableTreeNode("Settings")); api.add(new DefaultMutableTreeNode("Precautions")); root.add(new DefaultMutableTreeNode("Lock and Exit"));
        JTree tree = new JTree(root);
        JPanel settings = new JPanel(new GridLayout(0, 1));
        JTextField portField = new JTextField("1"); JCheckBox readOnly = new JCheckBox("Read-Only API", true); JCheckBox apiLog = new JCheckBox("Create API message log file");
        settings.add(portField); settings.add(readOnly); settings.add(apiLog);
        JPanel precautions = new JPanel(); JCheckBox bypass = new JCheckBox("Bypass Order Precautions for API Orders"); precautions.add(bypass);
        JPanel exit = new JPanel(); JRadioButton restart = new JRadioButton("Auto restart"); JTextField time = new JTextField("00:00"); JRadioButton am = new JRadioButton("AM", true), pm = new JRadioButton("PM");
        ButtonGroup group = new ButtonGroup(); group.add(am); group.add(pm); exit.add(restart); exit.add(time); exit.add(am); exit.add(pm);
        JPanel center = new JPanel(new BorderLayout()); center.add(settings);
        tree.addTreeSelectionListener(event -> { String name = event.getPath().getLastPathComponent().toString(); center.removeAll(); center.add(name.equals("Precautions") ? precautions : name.equals("Lock and Exit") ? exit : settings); dialog.pack(); center.revalidate(); center.repaint(); });
        JButton ok = new JButton("OK");
        ok.addActionListener(event -> {
            require(portField.getText().equals(Integer.toString(port)), "The API port was not set");
            require(!readOnly.isSelected() && apiLog.isSelected() && bypass.isSelected() && restart.isSelected(), "The API and restart settings were not applied");
            require(time.getText().matches("[0-9]{2}:[0-9]{2}"), "The restart time has the wrong format");
            configured++; dialog.dispose();
            if ("port_change".equals(System.getenv("FIXTURE_SCENARIO"))) {
                JDialog confirm = new JDialog(main, "IBKR Gateway", true);
                JButton yes = new JButton("Yes"), no = new JButton("No");
                yes.addActionListener(e -> { confirm.dispose(); try { new java.io.File(System.getenv("FIXTURE_MARKER")).createNewFile(); } catch (Exception x) { throw new RuntimeException(x); } });
                no.addActionListener(e -> { System.err.println("The port change was refused"); System.exit(6); });
                JPanel buttons = new JPanel(); buttons.add(yes); buttons.add(no);
                confirm.add(new JLabel("Do you want to apply the new socket port setting?")); confirm.add(buttons, BorderLayout.SOUTH);
                confirm.pack(); confirm.setVisible(true);
            }
            if (configured == 1 && System.getenv("FIXTURE_SCENARIO") == null) SwingUtilities.invokeLater(GatewayFixture::notices);
        });
        dialog.add(tree, BorderLayout.WEST); dialog.add(center); dialog.add(ok, BorderLayout.SOUTH); dialog.pack(); dialog.setVisible(true);
    }

    static void warning() {
        JDialog dialog = new JDialog(main, "Market Order Confirmation", true);
        JCheckBox suppress = new JCheckBox("Don't display this message again.");
        JButton accept = new JButton("Accept and Continue");
        accept.addActionListener(event -> { require(suppress.isSelected(), "Warning suppression was not set"); warningAccepted = true; dialog.dispose(); });
        dialog.add(suppress); dialog.add(accept, BorderLayout.SOUTH); dialog.pack(); dialog.setVisible(true);
    }

    static void notices() {
        JDialog existing = new JDialog(main, "Existing session detected", true);
        JButton proceed = new JButton("Continue Login"); proceed.addActionListener(e -> existing.dispose()); existing.add(proceed); existing.pack(); existing.setVisible(true);
        JDialog handoff = new JDialog(main, "Trading Login Handoff", true);
        JButton keep = new JButton("Disconnect Other Session"); keep.addActionListener(e -> handoff.dispose()); handoff.add(keep); handoff.pack(); handoff.setVisible(true);
        JDialog incoming = new JDialog(main, "API", true);
        JButton yes = new JButton("Yes"); yes.addActionListener(e -> incoming.dispose()); incoming.add(new JLabel("Accept incoming connection")); incoming.add(yes, BorderLayout.SOUTH); incoming.pack(); incoming.setVisible(true);
        warning();
    }

    static void logs() {
        JDialog view = new JDialog(main, "View Logs", true);
        JButton export = new JButton("Export Today Logs..."); JButton cancel = new JButton("Cancel"); cancel.addActionListener(event -> view.dispose());
        export.addActionListener(event -> {
            JDialog filename = new JDialog(view, "Enter export filename", true); JButton open = new JButton("Open"); open.addActionListener(e -> filename.dispose()); filename.add(open); filename.pack(); filename.setVisible(true);
            JOptionPane.showMessageDialog(view, "Finished exporting logs"); logExported = true; logExportCount++;
        });
        view.add(export); view.add(cancel, BorderLayout.SOUTH); view.pack(); view.setVisible(true);
    }
}
