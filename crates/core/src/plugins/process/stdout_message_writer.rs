use std::io::Write;
use std::sync::Arc;

use anyhow::Result;

use super::communication::MessageWriter;
use super::messages::Message;

#[derive(Clone)]
pub struct StdoutMessageWriter {
  tx: Arc<crossbeam_channel::Sender<Message>>,
}

impl StdoutMessageWriter {
  pub fn new<TWrite: Write + Unpin + Send + 'static>(mut writer: MessageWriter<TWrite>) -> Self {
    let (tx, rx) = crossbeam_channel::unbounded::<Message>();

    // use a dedicated thread for writing messages
    tokio::task::spawn_blocking({
      move || {
        while let Ok(result) = rx.recv() {
          result.write(&mut writer).unwrap();
        }
      }
    });

    Self { tx: Arc::new(tx) }
  }

  pub fn send(&self, message: Message) -> Result<()> {
    Ok(self.tx.send(message)?)
  }
}
