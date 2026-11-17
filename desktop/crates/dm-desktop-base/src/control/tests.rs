// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

/// A new token takes away the port file a crash left, so the two are never read as a pair
/// that sends the new token to an old port (security audit, 2026-11-12).
#[test]
fn a_new_token_takes_away_a_port_file_left_behind() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("control.port"), "41234").unwrap();
    let token = fresh_token(home.path()).unwrap();
    assert!(!home.path().join("control.port").exists());
    assert_eq!(
        std::fs::read_to_string(home.path().join("control.token")).unwrap(),
        token
    );
    // And with none there, as at a first start.
    fresh_token(home.path()).unwrap();
}

#[test]
fn the_port_file_goes_as_dm_quits_unless_another_dm_wrote_it() {
    let home = tempfile::tempdir().unwrap();
    write_port(home.path(), 4711).unwrap();
    forget_port(home.path());
    assert!(!home.path().join("control.port").exists());

    write_port(home.path(), 4711).unwrap();
    std::fs::write(home.path().join("control.port"), "4712").unwrap();
    forget_port(home.path());
    assert!(home.path().join("control.port").exists(), "another dm's");
}

#[test]
fn a_token_is_made_once_kept_and_private() {
    let home = tempfile::tempdir().unwrap();
    let first = token(home.path()).unwrap();
    assert_eq!(first.len(), 64);
    assert_eq!(
        token(home.path()).unwrap(),
        first,
        "a second start made a new token"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.path().join("control.token"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn nothing_is_answered_before_the_token() {
    let (to_window, from_net) = mpsc::channel();
    let port = listen(0, "a".repeat(64), to_window, || {}).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"{\"id\":1,\"method\":\"commands.list\"}\n")
        .unwrap();
    let mut reply = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut reply)
        .unwrap();
    assert!(reply.contains("authenticate first"), "{reply}");
    assert!(
        from_net.try_recv().is_err(),
        "a request got through unauthenticated"
    );

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"{\"id\":1,\"method\":\"auth\",\"params\":{\"token\":\"wrong\"}}\n")
        .unwrap();
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).unwrap();
    assert!(reply.contains("\"ok\":false"), "{reply}");
}

fn auth(port: u16, token: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let line = json!({ "id": 1, "method": "auth", "params": { "token": token } });
    stream.write_all(format!("{line}\n").as_bytes()).unwrap();
    let mut reply = String::new();
    let _ = BufReader::new(stream).read_line(&mut reply);
    reply
}

#[test]
fn idle_connections_without_the_token_do_not_keep_the_owner_out() {
    /* Sixteen connections that said nothing held every slot for the
     * five-minute read timeout, and the owner's was closed unanswered. */
    let token = "b".repeat(64);
    let (to_window, _from_net) = mpsc::channel();
    let port = listen(0, token.clone(), to_window, || {}).unwrap();
    let idle: Vec<TcpStream> = (0..MAX_WAITING + 4)
        .map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap())
        .collect();
    std::thread::sleep(Duration::from_millis(200));
    let reply = auth(port, &token);
    assert!(reply.contains("\"authenticated\":true"), "{reply:?}");
    drop(idle);
}

#[test]
fn a_connection_that_does_not_authenticate_in_time_is_closed() {
    let (to_window, _from_net) = mpsc::channel();
    let port = listen(0, "c".repeat(64), to_window, || {}).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(AUTH_WITHIN * 3)).unwrap();
    let started = Instant::now();
    // A byte now and then, never a whole line: each read is quick, so
    // only a deadline for the whole of it ends this.
    let mut writer = stream.try_clone().unwrap();
    std::thread::spawn(move || {
        for _ in 0..40 {
            if writer.write_all(b"{").is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest);
    let took = started.elapsed();
    assert!(
        took < AUTH_WITHIN + Duration::from_secs(3),
        "held for {took:?}"
    );
}

#[test]
fn a_line_that_is_not_json_before_the_token_ends_the_connection() {
    let (to_window, _from_net) = mpsc::channel();
    let port = listen(0, "d".repeat(64), to_window, || {}).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(b"garbage\n").unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut reply = String::new();
    reader.read_line(&mut reply).unwrap();
    assert!(reply.contains("not JSON"), "{reply}");
    let _ = stream.write_all(b"garbage\n");
    let mut more = String::new();
    assert_eq!(
        reader.read_line(&mut more).unwrap_or(0),
        0,
        "answered again: {more}"
    );
}

/// What only reads is answered on the connection's thread, the window
/// neither woken nor handed it; anything else still goes to the window.
#[test]
fn a_reading_is_answered_on_the_port_without_the_window() {
    let token = "c".repeat(64);
    let (to_window, from_net) = mpsc::channel();
    let woken = Arc::new(AtomicUsize::new(0));
    let reads: Reads =
        Arc::new(|method, _, _| (method == "queue.list").then(|| Ok(json!([{ "id": 7 }]))));
    let port = {
        let woken = Arc::clone(&woken);
        listen_reading(
            0,
            token.clone(),
            to_window,
            move || {
                woken.fetch_add(1, Ordering::SeqCst);
            },
            Some(reads),
        )
        .unwrap()
    };
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut replies = BufReader::new(stream.try_clone().unwrap());
    let mut ask = |line: Value| {
        stream.write_all(format!("{line}\n").as_bytes()).unwrap();
        let mut reply = String::new();
        replies.read_line(&mut reply).unwrap();
        reply
    };
    let said = ask(json!({ "id": 1, "method": "auth", "params": { "token": token } }));
    assert!(said.contains("\"ok\":true"), "{said}");

    let said = ask(json!({ "id": 2, "method": "queue.list" }));
    assert!(said.contains("\"id\":7"), "{said}");
    assert_eq!(woken.load(Ordering::SeqCst), 0, "the window was woken");
    assert!(from_net.try_recv().is_err(), "the window was handed it");

    // Anything else: the window's, woken for it.
    let window = std::thread::spawn(move || {
        let request = from_net.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(request.method, "commands.list");
        request.reply.send(Ok(json!("the window's"))).unwrap();
    });
    let said = ask(json!({ "id": 3, "method": "commands.list" }));
    window.join().unwrap();
    assert!(said.contains("the window's"), "{said}");
    assert_eq!(woken.load(Ordering::SeqCst), 1);
}

#[test]
fn a_request_waiting_as_dm_quits_is_told_dm_is_closing() {
    let (_reply, answered) = mpsc::channel();
    let closing = AtomicBool::new(true);
    let started = Instant::now();
    let answer = wait_for(&answered, &closing, Duration::from_secs(30));
    assert_eq!(answer, Err("dm is closing".to_string()));
    assert!(started.elapsed() < Duration::from_secs(5));

    // And when the window that would have answered is gone.
    let (reply, answered) = mpsc::channel::<Result<Value, String>>();
    drop(reply);
    let closing = AtomicBool::new(false);
    let answer = wait_for(&answered, &closing, Duration::from_secs(30));
    assert_eq!(answer, Err("dm is closing".to_string()));
}
