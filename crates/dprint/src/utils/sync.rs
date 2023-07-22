use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::VecDeque;
use std::future::Future;
use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

// todo(THIS PR): unit tests

pub struct MutexGuard<'a, T> {
  state: RefMut<'a, T>,
  _permit: SemaphorePermit,
}

impl<'a, T> Deref for MutexGuard<'a, T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    &self.state
  }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.state.deref_mut()
  }
}

pub struct AsyncMutex<T> {
  state: RefCell<T>,
  semaphore: Rc<Semaphore>,
}

impl<T> Default for AsyncMutex<T>
where
  T: Default,
{
  fn default() -> Self {
    Self::new(Default::default())
  }
}

impl<T> AsyncMutex<T> {
  pub fn new(value: T) -> Self {
    Self {
      state: RefCell::new(value),
      semaphore: Rc::new(Semaphore::new(1)),
    }
  }

  pub async fn lock(&self) -> MutexGuard<'_, T> {
    let permit = self.semaphore.acquire().await.unwrap();
    let state = self.state.borrow_mut();
    MutexGuard { state, _permit: permit }
  }
}

struct SemaphoreState {
  closed: bool,
  permits: usize,
  wakers: VecDeque<Waker>,
}

pub struct SemaphorePermit(Rc<Semaphore>);

impl Drop for SemaphorePermit {
  fn drop(&mut self) {
    self.0.release();
  }
}

pub struct Semaphore {
  state: RefCell<SemaphoreState>,
}

impl Semaphore {
  pub fn new(permits: usize) -> Self {
    Self {
      state: RefCell::new(SemaphoreState {
        closed: false,
        permits,
        wakers: VecDeque::new(),
      }),
    }
  }

  pub fn acquire(self: &Rc<Self>) -> impl Future<Output = Result<SemaphorePermit, ()>> {
    AcquireFuture { semaphore: self.clone() }
  }

  pub fn add_permits(&self, amount: usize) {
    let wakers = {
      let mut wakers = Vec::with_capacity(amount);
      let mut state = self.state.borrow_mut();
      state.permits += amount;

      for _ in 0..amount {
        match state.wakers.pop_front() {
          Some(waker) => wakers.push(waker),
          None => break,
        }
      }
      wakers
    };
    for waker in wakers {
      waker.wake();
    }
  }

  pub fn close(&self) {
    let wakers = {
      let mut state = self.state.borrow_mut();
      state.closed = true;
      std::mem::take(&mut state.wakers)
    };
    for waker in wakers {
      waker.wake();
    }
  }

  fn release(&self) {
    let maybe_waker = {
      let mut state = self.state.borrow_mut();

      state.permits += 1;
      state.wakers.pop_front()
    };

    if let Some(waker) = maybe_waker {
      waker.wake();
    }
  }
}

struct AcquireFuture {
  semaphore: Rc<Semaphore>,
}

impl Future for AcquireFuture {
  type Output = Result<SemaphorePermit, ()>;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let mut state = self.semaphore.state.borrow_mut();

    if state.closed {
      Poll::Ready(Err(()))
    } else if state.permits > 0 {
      state.permits -= 1;
      Poll::Ready(Ok(SemaphorePermit(self.semaphore.clone())))
    } else {
      state.wakers.push_back(cx.waker().clone());
      Poll::Pending
    }
  }
}
