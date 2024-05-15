use log::Record;
use log4rs::append::Append;
use reqwest::Client;
use serde_json::json;
use std::fmt;

use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;

pub struct BetterStackAppender {
    sender: mpsc::Sender<LogMessage>,
}

#[derive(Debug, serde::Serialize)]
struct LogMessage {
    timestamp: String,
    level: String,
    target: String,
    thread: Option<String>,
    message: String,
    module_path: Option<String>,
    file: Option<String>,
    line: Option<u32>,
}

#[derive(serde::Serialize)]
struct ThreadInfo {
    id: String,
    name: Option<String>,
}

impl fmt::Debug for BetterStackAppender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BetterStackAppender")
            .field("sender", &self.sender)
            .finish()
    }
}

impl Append for BetterStackAppender {
    fn append(&self, record: &Record) -> anyhow::Result<()> {
        let log_message = build_log_message(record);

        let _ = self.sender.try_send(log_message).ok(); // TODO Proper error handling
        Ok(())
    }

    fn flush(&self) {
        // Handle flushing if necessary
    }
}

impl BetterStackAppender {
    pub fn new(ingest_url: String, source_token: String) -> BetterStackAppender {
        let (sender, mut receiver) = mpsc::channel(100); // TODO should this be configurable?
        let client = Client::new();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(3));
            let mut batch: Vec<LogMessage> = Vec::new();

            loop {
                tokio::select! {
                    Some(msg) = receiver.recv() => {
                        batch.push(msg);
                        if batch.len() >= 1000 { // TODO should this be configurable?
                            Self::send_batch(&client, &ingest_url, &source_token, &mut batch).await;
                        }
                    }
                    _ = interval.tick() => {
                        if !batch.is_empty() {
                            Self::send_batch(&client, &ingest_url, &source_token, &mut batch).await;
                        }
                    }
                }
            }
        });

        BetterStackAppender { sender }
    }

    async fn send_batch(client: &Client, url: &str, token: &str, batch: &mut Vec<LogMessage>) {
        let json = json!(batch);
        let _ = client.post(url).bearer_auth(token).json(&json).send().await;

        batch.clear();
    }
}

fn build_log_message(record: &Record) -> LogMessage {
    let level_string = format!("{:<5}", record.level().to_string());

    let target = if !record.target().is_empty() {
        record.target().to_string()
    } else {
        record.module_path().unwrap_or_default().to_string()
    };

    let thread_info: Option<ThreadInfo> = {
        #[cfg(feature = "threads")]
        {
            let thread = std::thread::current();
            let thread_name = {
                #[cfg(feature = "nightly")]
                {
                    thread
                        .name()
                        .unwrap_or(&format!("{}", thread.id().as_u64()))
                }
                #[cfg(not(feature = "nightly"))]
                {
                    thread.name().unwrap_or("?").to_string()
                }
            };
            Some(ThreadInfo {
                id: format!("{:?}", thread.id()),
                name: Some(thread_name),
            })
        }
        #[cfg(not(feature = "threads"))]
        None
    };

    let timestamp: String = {
        #[cfg(feature = "timestamps")]
        match self.timestamps {
            Timestamps::None => "".to_string(),
            Timestamps::Local => format!(
                "{} ",
                OffsetDateTime::now_local()
                    .unwrap()
                    .format(&self.timestamps_format.unwrap_or(TIMESTAMP_FORMAT_OFFSET))
                    .unwrap()
            ),
            Timestamps::Utc => format!(
                "{} ",
                OffsetDateTime::now_utc()
                    .format(&self.timestamps_format.unwrap_or(TIMESTAMP_FORMAT_UTC))
                    .unwrap()
            ),
            Timestamps::UtcOffset(offset) => format!(
                "{} ",
                OffsetDateTime::now_utc()
                    .to_offset(offset)
                    .format(&self.timestamps_format.unwrap_or(TIMESTAMP_FORMAT_OFFSET))
                    .unwrap()
            ),
        }
        #[cfg(not(feature = "timestamps"))]
        "".to_string()
    };

    LogMessage {
        timestamp,
        level: level_string,
        target,
        thread: thread_info.map(|ti| ti.name.unwrap_or_default()),
        message: format!("{}", record.args()),
        module_path: record.module_path().map(ToString::to_string),
        file: record.file().map(ToString::to_string),
        line: record.line(),
    }
}
