use std::io::ErrorKind;
use std::io::Write;
use std::sync::Arc;

use crate::communication::MessageWriter;

use super::Poisoner;

pub trait Message: std::fmt::Debug + Send + Sync + 'static {
  fn write<TWrite: Write + Unpin>(&self, writer: &mut MessageWriter<TWrite>) -> std::io::Result<()>;
}

struct SingleThreadMessageWriterOptions<TWrite: Write + Unpin> {
  pub writer: MessageWriter<TWrite>,
  pub panic_on_write_fail: bool,
  pub on_exit: Box<dyn Fn() + Send + 'static>,
}

/// Writes messages on a separate thread.
pub struct SingleThreadMessageWriter<TMessage: Message> {
  tx: Arc<crossbeam_channel::Sender<TMessage>>,
}

// the #[derive(Clone)] macro wasn't working with the type parameter properly
// https://github.com/rust-lang/rust/issues/26925
impl<TMessage: Message> Clone for SingleThreadMessageWriter<TMessage> {
  fn clone(&self) -> Self {
    Self { tx: self.tx.clone() }
  }
}

impl<TMessage: Message> SingleThreadMessageWriter<TMessage> {
  pub fn for_stdout<TWrite: Write + Unpin + Send + 'static>(writer: MessageWriter<TWrite>) -> Self {
    Self::new(SingleThreadMessageWriterOptions {
      writer,
      panic_on_write_fail: true,
      on_exit: Box::new(|| {}),
    })
  }

  pub fn for_stdin<TWrite: Write + Unpin + Send + 'static>(writer: MessageWriter<TWrite>, poisoner: Poisoner) -> Self {
    Self::new(SingleThreadMessageWriterOptions {
      writer,
      panic_on_write_fail: false,
      on_exit: Box::new(move || {
        poisoner.poison();
      }),
    })
  }

  fn new<TWrite: Write + Unpin + Send + 'static>(mut opts: SingleThreadMessageWriterOptions<TWrite>) -> Self {
    let (tx, rx) = crossbeam_channel::unbounded::<TMessage>();

    // use a dedicated thread for writing messages
    tokio::task::spawn_blocking({
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
        (opts.on_exit)();
      }
    });

    Self { tx: Arc::new(tx) }
  }

  pub fn send(&self, message: TMessage) -> std::io::Result<()> {
    self.tx.send(message).map_err(|err| std::io::Error::new(ErrorKind::BrokenPipe, err))
  }
}
