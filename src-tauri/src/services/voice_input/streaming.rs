use super::audio::Recorder;
use crate::models::model::{AccountProvider, AccountSource};
use crate::services::plugin_accounts::stream;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

pub struct Session {
    finish: oneshot::Sender<Vec<u8>>,
    result: tauri::async_runtime::JoinHandle<Result<String, String>>,
}

impl Session {
    pub fn start(account_id: String, recorder: Recorder, cancel: CancellationToken) -> Self {
        let (finish, ending) = oneshot::channel();
        let result = tauri::async_runtime::spawn(async move {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err("voice.error.cancelled".into()),
                result = run(account_id, recorder, ending) => result,
            }
        });
        Self { finish, result }
    }

    pub async fn finish(self, wav: &[u8]) -> Result<String, String> {
        let _ = self.finish.send(wav[44..].to_vec());
        self.result
            .await
            .map_err(|_| "voice.error.transcribe".to_string())?
    }
}

async fn send_pcm(
    sink: &mut SplitSink<stream::Socket, Message>,
    bytes: &[u8],
) -> Result<(), String> {
    for chunk in bytes.chunks(3200) {
        tokio::time::timeout(
            Duration::from_secs(5),
            sink.send(Message::Binary(chunk.to_vec().into())),
        )
        .await
        .map_err(|_| "voice.error.transcribe".to_string())?
        .map_err(|_| "voice.error.transcribe".to_string())?;
    }
    Ok(())
}

async fn run(
    account_id: String,
    recorder: Recorder,
    ending: oneshot::Receiver<Vec<u8>>,
) -> Result<String, String> {
    let source = AccountSource {
        provider: AccountProvider::WorkBuddy,
        account_id,
    };
    let (socket, ready) = stream::connect("workbuddy", &source.account_id)
        .await
        .map_err(|_| "voice.error.transcribe".to_string())?;
    transfer(
        socket,
        ready,
        move || {
            let recorder = recorder.clone();
            async move {
                tokio::task::spawn_blocking(move || recorder.chunk())
                    .await
                    .map_err(|_| "voice.error.microphone".to_string())?
            }
        },
        ending,
    )
    .await
}

async fn transfer<F, Fut>(
    socket: stream::Socket,
    ready: stream::Event,
    mut read_chunk: F,
    mut ending: oneshot::Receiver<Vec<u8>>,
) -> Result<String, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
{
    if ready.final_result {
        return Err("voice.error.transcribe".into());
    }
    log::info!("[VoiceInput] realtime connected");
    let (mut sink, mut events) = socket.split();
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut sent = 0;
    let mut finishing = false;
    let mut capturing = true;
    let mut deadline = tokio::time::Instant::now();
    loop {
        tokio::select! {
            pcm = &mut ending, if !finishing => {
                let pcm = pcm.map_err(|_| "voice.error.cancelled".to_string())?;
                let tail = pcm.get(sent..).ok_or("voice.error.transcribe")?;
                send_pcm(&mut sink, tail).await?;
                sent += tail.len();
                tokio::time::timeout(Duration::from_secs(5), sink.send(Message::Binary(Vec::new().into())))
                    .await.map_err(|_| "voice.error.transcribe".to_string())?
                    .map_err(|_| "voice.error.transcribe".to_string())?;
                log::info!("[VoiceInput] realtime finished sending pcm_bytes={sent}");
                finishing = true;
                deadline = tokio::time::Instant::now() + Duration::from_secs(25);
            }
            _ = interval.tick(), if !finishing && capturing => {
                let chunk = read_chunk().await;
                match chunk {
                    Ok(chunk) => {
                        send_pcm(&mut sink, &chunk).await?;
                        sent += chunk.len();
                    }
                    Err(error) if error == "voice.error.idle" => capturing = false,
                    Err(error) => return Err(error),
                }
            }
            message = events.next() => {
                let message = message.ok_or("voice.error.transcribe")?
                    .map_err(|_| "voice.error.transcribe".to_string())?;
                if let Some(event) = stream::event(message).map_err(|_| "voice.error.transcribe".to_string())? {
                    if event.final_result {
                        if !finishing { return Err("voice.error.transcribe".into()); }
                        log::info!("[VoiceInput] realtime final characters={}", event.text.chars().count());
                        let _ = tokio::time::timeout(Duration::from_secs(2), sink.close()).await;
                        return if event.text.trim().is_empty() { Err("voice.error.noTranscript".into()) } else { Ok(event.text) };
                    }
                    if !event.text.is_empty() {
                        log::debug!("[VoiceInput] realtime partial characters={}", event.text.chars().count());
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline), if finishing => return Err("voice.error.transcribe".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

    async fn fixture(final_text: &'static str) -> (Value, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/voice/fixture", listener.local_addr().unwrap());
        #[allow(clippy::result_large_err)] // The websocket callback fixes this signature.
        let task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(
                tcp,
                |request: &Request, response: Response| {
                    assert_eq!(request.uri().path(), "/voice/fixture");
                    assert_eq!(request.headers()["authorization"], "Bearer fixture-token");
                    Ok(response)
                },
            )
            .await
            .unwrap();
            socket
                .send(Message::Text(
                    json!({"text":"","finalResult":false}).to_string().into(),
                ))
                .await
                .unwrap();
            let mut bytes = Vec::new();
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(chunk) if chunk.is_empty() => {
                        socket
                            .send(Message::Text(
                                json!({"text":"修订的中间结果","finalResult":false})
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .unwrap();
                        socket
                            .send(Message::Text(
                                json!({"text":final_text,"finalResult":true})
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .unwrap();
                        let closing = tokio::time::timeout(Duration::from_secs(2), socket.next())
                            .await
                            .unwrap();
                        assert!(
                            matches!(closing, Some(Ok(Message::Close(_)))),
                            "final result must close the WebSocket cleanly"
                        );
                        break;
                    }
                    Message::Binary(chunk) => {
                        assert!(chunk.len() <= 3200);
                        assert_eq!(chunk.len() % 2, 0);
                        bytes.extend_from_slice(&chunk);
                        socket
                            .send(Message::Text(
                                json!({"text":"早期结果","finalResult":false})
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .unwrap();
                    }
                    Message::Close(_) => break,
                    _ => panic!("expected binary PCM, not a WAV header or start JSON"),
                }
            }
            bytes
        });
        (json!({"endpoint":endpoint}), task)
    }

    #[tokio::test]
    async fn stream_flushes_tail_and_uses_final_full_text_without_concatenating_revisions() {
        let (credentials, server) = fixture("最终的完整长句，包括最后一句。").await;
        let (socket, ready) = stream::connect_endpoint(
            credentials["endpoint"].as_str().unwrap(),
            "Bearer fixture-token",
        )
        .await
        .unwrap();
        let pcm: Vec<_> = (0..10002).map(|index| (index % 251) as u8).collect();
        let expected = pcm.clone();
        let mut chunks = VecDeque::from([pcm[..4000].to_vec(), pcm[4000..8000].to_vec()]);
        let (ending, ended) = oneshot::channel();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            ending.send(pcm).unwrap();
        });
        let text = transfer(
            socket,
            ready,
            || std::future::ready(Ok(chunks.pop_front().unwrap_or_default())),
            ended,
        )
        .await
        .unwrap();
        assert_eq!(text, "最终的完整长句，包括最后一句。");
        assert_eq!(server.await.unwrap(), expected);
    }

    #[tokio::test]
    async fn empty_final_does_not_promote_an_interim_transcript_to_success() {
        let (credentials, server) = fixture("").await;
        let (socket, ready) = stream::connect_endpoint(
            credentials["endpoint"].as_str().unwrap(),
            "Bearer fixture-token",
        )
        .await
        .unwrap();
        let (ending, ended) = oneshot::channel();
        ending.send(vec![0; 3200]).unwrap();
        let result = transfer(socket, ready, || std::future::ready(Ok(Vec::new())), ended).await;
        assert_eq!(result, Err("voice.error.noTranscript".into()));
        assert_eq!(server.await.unwrap().len(), 3200);
    }

    #[tokio::test]
    async fn dropping_a_cancelled_stream_closes_it_without_a_finish_or_extra_audio() {
        let (credentials, server) = fixture("must not finish").await;
        let (socket, ready) = stream::connect_endpoint(
            credentials["endpoint"].as_str().unwrap(),
            "Bearer fixture-token",
        )
        .await
        .unwrap();
        let (_ending, ended) = oneshot::channel();
        let result = tokio::time::timeout(
            Duration::from_millis(50),
            transfer(
                socket,
                ready,
                || std::future::ready(Ok(vec![1; 3200])),
                ended,
            ),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(server.await.unwrap().len(), 3200);
    }

    #[test]
    fn stream_errors_never_expose_the_upstream_message() {
        let error = stream::event(Message::Text(
            json!({"code":7,"message":"fixture-private-upstream"})
                .to_string()
                .into(),
        ))
        .err()
        .unwrap();
        assert!(!error.to_string().contains("fixture-private"));
    }

    #[tokio::test]
    #[ignore = "requires an explicitly selected saved WorkBuddy account and synthetic WAV"]
    async fn live_workbuddy_stream_keeps_the_end_of_synthetic_speech() {
        let account_id =
            std::env::var("ECHOBIRD_VOICE_TEST_ACCOUNT").expect("explicit account required");
        let wav = std::fs::read(
            std::env::var("ECHOBIRD_VOICE_TEST_WAV").expect("synthetic WAV required"),
        )
        .unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16000);
        let source = AccountSource {
            provider: AccountProvider::WorkBuddy,
            account_id,
        };
        let (socket, ready) = stream::connect("workbuddy", &source.account_id)
            .await
            .unwrap();
        let pcm = wav[44..].to_vec();
        let mut chunks: VecDeque<_> = pcm.chunks(3200).map(<[u8]>::to_vec).collect();
        let duration = Duration::from_millis(pcm.len() as u64 * 1000 / 32000 + 200);
        let (ending, ended) = oneshot::channel();
        tokio::spawn(async move {
            tokio::time::sleep(duration).await;
            ending.send(pcm).unwrap();
        });
        let text = transfer(
            socket,
            ready,
            || std::future::ready(Ok(chunks.pop_front().unwrap_or_default())),
            ended,
        )
        .await
        .unwrap();
        println!("Synthetic streaming transcription: {text}");
        assert!(
            text.contains("翻译"),
            "final synthetic speech sentence was lost"
        );
    }
}
