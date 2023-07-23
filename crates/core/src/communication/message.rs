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
  tx: crossbeam_channel::Sender<TMessage>,
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
    let (tx, rx) = crossbeam_channel::unbounded::<TMessage>();

    // use a dedicated thread for writing messages
    crate::async_runtime::spawn_blocking({
      move || {
        while let Ok(result) = rx.recv() {
          if let Err(err) = result.write(&mut opts.writer) {
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
    self.tx.send(message).map_err(|err| std::io::Error::new(ErrorKind::BrokenPipe, err))
  }
}
