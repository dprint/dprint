use anyhow::Result;
use tokio::io::AsyncWrite;
use tokio::sync::mpsc;

use super::communication::MessageWriter;
use super::messages::Message;

#[derive(Clone)]
pub struct StdoutMessageWriter {
  tx: mpsc::UnboundedSender<Message>,
}

impl StdoutMessageWriter {
  pub fn new<TWrite: AsyncWrite + Unpin + Send + 'static>(mut writer: MessageWriter<TWrite>) -> Self {
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // use a dedicated task for writing messages
    tokio::task::spawn({
      async move {
        while let Some(result) = rx.recv().await {
          result.write(&mut writer).await.unwrap();
        }
      }
    });

    Self { tx }
  }

  pub fn send(&self, message: Message) -> Result<()> {
    Ok(self.tx.send(message)?)
  }
}
