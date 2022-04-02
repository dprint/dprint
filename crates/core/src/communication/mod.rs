use tokio_util::sync::CancellationToken;

use crate::plugins::BoxFuture;

mod message;
mod reader_writer;
mod utils;

pub use message::*;
pub use reader_writer::*;
pub use utils::*;

impl crate::plugins::CancellationToken for CancellationToken {
  fn is_cancelled(&self) -> bool {
    self.is_cancelled()
  }

  fn wait_cancellation(&self) -> BoxFuture<'static, ()> {
    let token = self.clone();
    Box::pin(async move { token.cancelled().await })
  }
}
