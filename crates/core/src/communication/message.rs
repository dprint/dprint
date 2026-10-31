use std::io::ErrorKind;
use std::io::Write;

use crate::communication::MessageWriter;

pub trait Message: std::fmt::Debug + Send + Sync + 'static {
  fn write<TWrite: Write + Unpin>(&self, writer: &mut MessageWriter<TWrite>) -> std::io::Result<()>;
}

struct SingleThreadMessageWriterOptions<TWrite: Write + Unpin> {
  pub writer: MessageWriter<TWrite>,
  pub panic_on_write_fail: bool,
}

/// Writes messages on a separate thread.
pub struct SingleThreadMessageWriter<TMessage: Message> {
  tx: crossbeam_channel::Sender<WriterItem<TMessage>>,
}

enum WriterItem<TMessage: Message> {
  Message(TMessage),
  /// Notifies once the messages sent before it were written.
  Flush(tokio::sync::oneshot::Sender<()>),
}

impl<TMessage: Message> SingleThreadMessageWriter<TMessage> {
  pub fn for_stdout<TWrite: Write + Unpin + Send + 'static>(writer: MessageWriter<TWrite>) -> Self {
    Self::new(SingleThreadMessageWriterOptions {
      writer,
      panic_on_write_fail: true,
    })
  }

  pub fn for_stdin<TWrite: Write + Unpin + Send + 'static>(writer: MessageWriter<TWrite>) -> Self {
    Self::new(SingleThreadMessageWriterOptions {
      writer,
      panic_on_write_fail: false,
    })
  }

  fn new<TWrite: Write + Unpin + Send + 'static>(mut opts: SingleThreadMessageWriterOptions<TWrite>) -> Self {
    let (tx, rx) = crossbeam_channel::unbounded::<WriterItem<TMessage>>();

    // use a dedicated thread for writing messages
    crate::async_runtime::spawn_blocking({
      move || {
        while let Ok(item) = rx.recv() {
          let message = match item {
            WriterItem::Message(message) => message,
            WriterItem::Flush(sender) => {
              let _ = sender.send(());
              continue;
            }
          };
          if let Err(err) = message.write(&mut opts.writer) {
            if opts.panic_on_write_fail {
              panic!("{:#}", err);
            } else {
              break;
            }
          }
        }
      }
    });

    Self { tx }
  }

  pub fn send(&self, message: TMessage) -> std::io::Result<()> {
    self
      .tx
      .send(WriterItem::Message(message))
      .map_err(|_| std::io::Error::new(ErrorKind::BrokenPipe, "The message writer stopped."))
  }

  /// Waits for the messages that were sent to be written, which is useful
  /// before exiting the process. This resolves right away when writing
  /// failed.
  pub async fn flush(&self) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    if self.tx.send(WriterItem::Flush(sender)).is_ok() {
      // the writer stopped when this errors
      let _ = receiver.await;
    }
  }
}
