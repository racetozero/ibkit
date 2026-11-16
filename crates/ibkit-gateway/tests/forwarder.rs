use ibkit_gateway::forwarder::{forward_api, supervise};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};

#[tokio::test]
async fn forwarding_preserves_bytes_and_recovers_after_a_refused_target() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    drop(target);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let relay = tokio::spawn(async move { forward_api(listener, target_address, &None).await });
    let mut refused = TcpStream::connect(address).await.unwrap();
    let mut byte = [0];
    assert!(matches!(
        timeout(Duration::from_secs(4), refused.read(&mut byte)).await,
        Ok(Ok(0) | Err(_))
    ));
    let target = TcpListener::bind(target_address).await.unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..70 {
            let (mut stream, peer) = target.accept().await.unwrap();
            assert!(peer.ip().is_loopback());
            let mut request = Vec::new();
            stream.read_to_end(&mut request).await.unwrap();
            assert_eq!(request, b"API\0\xff\0request");
            stream.write_all(b"reply\0\xff").await.unwrap();
        }
    });
    for _ in 0..70 {
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all(b"API\0\xff\0request").await.unwrap();
        client.shutdown().await.unwrap();
        let mut reply = Vec::new();
        timeout(Duration::from_secs(4), client.read_to_end(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reply, b"reply\0\xff");
    }
    server.await.unwrap();
    relay.abort();
    let _ = relay.await;
    assert!(TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn supervisor_binds_once_the_address_is_free() {
    let busy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = busy.local_addr().unwrap();
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = target.local_addr().unwrap().port();
    let supervisor = tokio::spawn(supervise(address, port, None));
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(busy);
    let mut client = timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(client) = TcpStream::connect(address).await {
                return client;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    client.write_all(b"ping").await.unwrap();
    let (mut stream, _) = timeout(Duration::from_secs(5), target.accept())
        .await
        .unwrap()
        .unwrap();
    let mut buffer = [0; 4];
    stream.read_exact(&mut buffer).await.unwrap();
    assert_eq!(&buffer, b"ping");
    supervisor.abort();
}

#[test]
fn allowed_sources_match_addresses_and_ranges() {
    use ibkit_gateway::forwarder::allowed;
    let sources = Some(vec![
        ("10.0.0.0".parse().unwrap(), 8),
        ("192.168.1.20".parse().unwrap(), 32),
        ("fd00::".parse().unwrap(), 8),
    ]);
    for (address, expected) in [
        ("10.1.2.3", true),
        ("11.0.0.1", false),
        ("192.168.1.20", true),
        ("192.168.1.21", false),
        ("fd12::1", true),
        ("::ffff:10.0.0.9", true),
        ("2001:db8::1", false),
    ] {
        assert_eq!(
            allowed(&sources, address.parse().unwrap()),
            expected,
            "{address}"
        );
    }
    assert!(allowed(&None, "203.0.113.9".parse().unwrap()));
}
