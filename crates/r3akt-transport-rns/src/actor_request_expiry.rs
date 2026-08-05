use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::*;

#[test]
fn zmq_data_plane_drops_expired_requests_before_session_startup() {
    let (send_tx, send_rx) = mpsc::sync_channel(1);
    let (control_tx, control_rx) = mpsc::sync_channel(1);
    let (response_tx, response_rx) = mpsc::channel();
    send_tx
        .send(ZmqSdkActorRequest {
            payload: ZmqSdkActorPayload::Status("stale-status".to_string()),
            response: response_tx,
            queued_at: Instant::now()
                .checked_sub(Duration::from_secs(2))
                .expect("instant subtraction"),
        })
        .expect("queue stale request");
    drop(send_tx);
    drop(control_tx);

    let mut config = rch_local_zmq_pipeline_config("tcp://127.0.0.1:1", "tcp://127.0.0.1:2");
    config.request_timeout = Duration::from_millis(50);
    let metrics = ZmqDataPlaneMetrics::default();
    run_zmq_data_plane_actor(&config, &send_rx, &control_rx, &metrics);

    let stats = metrics.snapshot();
    assert_eq!(stats.expired_total, 1);
    assert_eq!(stats.queue_depth, 0);
    assert!(matches!(
        response_rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}
