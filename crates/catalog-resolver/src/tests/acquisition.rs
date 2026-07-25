//! Raw Rust HTTPS acquisition port behavior tests.

use crate::{
    AcquisitionRefusal, CancellationToken, HttpsPortError, RustHttpsAcquisitionConfig,
    RustHttpsAcquisitionPort, https::HttpsWaitError,
};

use super::*;

#[test]
fn rust_https_port_refuses_literal_private_target_before_connect() {
    let port = RustHttpsAcquisitionPort::new(RustHttpsAcquisitionConfig::default())
        .expect("Rust HTTPS port");
    let completion = HttpsAcquisitionCompletion::pending();
    let operation = port
        .start_fetch(
            HttpsFetchRequest {
                url: Arc::from("https://127.0.0.1/artifact"),
                maximum_bytes: 1_024,
            },
            completion.clone(),
        )
        .expect("start");
    let result = completion.wait(&CancellationToken::default());
    operation.cancel();
    assert!(matches!(
        result,
        Err(HttpsWaitError::Port(HttpsPortError::Refused {
            reason: AcquisitionRefusal::NonPublicAddress { .. }
        }))
    ));
}
