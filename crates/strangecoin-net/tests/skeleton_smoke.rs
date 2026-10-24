use strangecoin_net::NetError;

#[test]
fn net_error_protocol_variant_is_available() {
    let err = NetError::Protocol("bad message".to_string());
    assert!(matches!(err, NetError::Protocol(_)));
    assert_eq!(err.to_string(), "protocol error: bad message");
}

#[test]
fn io_error_converts_to_transport() {
    let io = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset");
    let err: NetError = io.into();
    assert!(matches!(err, NetError::Transport(_)));
    assert!(err.to_string().contains("transport error"));
}
