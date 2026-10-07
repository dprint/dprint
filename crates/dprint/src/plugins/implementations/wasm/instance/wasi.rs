use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Result;
use rand::RngCore;
use wasmtime::Caller;
use wasmtime::Extern;
use wasmtime::ExternType;
use wasmtime::Memory;
use wasmtime::Val;
use wasmtime::ValType;

use super::Linker;
use super::Store;
use super::WasmHostState;

const MODULE: &str = "wasi_snapshot_preview1";

const ERRNO_SUCCESS: i32 = 0;
const ERRNO_BADF: i32 = 8;
const ERRNO_FAULT: i32 = 21;
const ERRNO_INVAL: i32 = 28;
const ERRNO_NOSYS: i32 = 52;
const ERRNO_SPIPE: i32 = 70;

const CLOCK_REALTIME: i32 = 0;
const CLOCK_MONOTONIC: i32 = 1;
/// The clocks are only precise to a millisecond in order to not hand plugins
/// a high resolution timer.
const CLOCK_RESOLUTION_NANOS: u64 = 1_000_000;

const EVENTTYPE_CLOCK: u8 = 0;
const EVENTTYPE_FD_READ: u8 = 1;
const EVENTTYPE_FD_WRITE: u8 = 2;
const SUBCLOCKFLAGS_ABSTIME: u16 = 1;
const SUBSCRIPTION_SIZE: u32 = 48;
const EVENT_SIZE: usize = 32;
/// Maximum number of subscriptions a plugin may provide in a single poll.
const MAX_SUBSCRIPTIONS: u32 = 1024;

const FILETYPE_CHARACTER_DEVICE: u8 = 2;
const RIGHTS_FD_READ: u64 = 1 << 1;
const RIGHTS_FD_WRITE: u64 = 1 << 6;

/// Maximum number of buffers a plugin may provide in a single write.
const MAX_IOVS: u32 = 1024;
/// Maximum number of bytes logged for a single write. Anything past this is
/// reported back to the plugin as not written.
const MAX_WRITE_BYTES: usize = 64 * 1024;

/// Adds a sandboxed subset of WASI preview 1 to the linker so that plugins
/// compiled with a toolchain that needs a WASI libc (ex. .NET, Go) can be
/// instantiated.
///
/// Nothing here gives a plugin a way to change anything on the host. The only
/// things provided are the clocks, sleeping, random bytes, and writing text
/// to stdout/stderr (which gets logged). There are no environment variables,
/// arguments, or preopened directories, so every file system function fails.
pub fn add_wasi_imports(linker: &mut Linker) -> Result<()> {
  // capabilities
  linker.func_wrap(MODULE, "clock_res_get", clock_res_get)?;
  linker.func_wrap(MODULE, "clock_time_get", clock_time_get)?;
  linker.func_wrap(MODULE, "poll_oneoff", poll_oneoff)?;
  linker.func_wrap(MODULE, "random_get", random_get)?;
  linker.func_wrap(MODULE, "fd_write", fd_write)?;
  linker.func_wrap(MODULE, "sched_yield", || -> i32 { ERRNO_SUCCESS })?;
  linker.func_wrap(MODULE, "proc_exit", proc_exit)?;

  // empty environment
  linker.func_wrap(MODULE, "args_sizes_get", write_two_zeros)?;
  linker.func_wrap(MODULE, "args_get", |_: i32, _: i32| -> i32 { ERRNO_SUCCESS })?;
  linker.func_wrap(MODULE, "environ_sizes_get", write_two_zeros)?;
  linker.func_wrap(MODULE, "environ_get", |_: i32, _: i32| -> i32 { ERRNO_SUCCESS })?;

  // standard streams
  linker.func_wrap(MODULE, "fd_fdstat_get", fd_fdstat_get)?;
  linker.func_wrap(MODULE, "fd_filestat_get", fd_filestat_get)?;
  linker.func_wrap(MODULE, "fd_read", fd_read)?;
  linker.func_wrap(MODULE, "fd_seek", |fd: i32, _: i64, _: i32, _: i32| -> i32 {
    if is_std_stream(fd) { ERRNO_SPIPE } else { ERRNO_BADF }
  })?;
  linker.func_wrap(MODULE, "fd_close", |fd: i32| -> i32 {
    if is_std_stream(fd) { ERRNO_SUCCESS } else { ERRNO_BADF }
  })?;

  // no file system. Returning "bad file descriptor" from `fd_prestat_get` tells
  // the libc that there are no preopened directories.
  linker.func_wrap(MODULE, "fd_prestat_get", |_: i32, _: i32| -> i32 { ERRNO_BADF })?;
  linker.func_wrap(MODULE, "fd_prestat_dir_name", |_: i32, _: i32, _: i32| -> i32 { ERRNO_BADF })?;
  linker.func_wrap(MODULE, "fd_advise", |_: i32, _: i64, _: i64, _: i32| -> i32 { ERRNO_BADF })?;
  linker.func_wrap(MODULE, "fd_pread", |_: i32, _: i32, _: i32, _: i64, _: i32| -> i32 { ERRNO_BADF })?;
  linker.func_wrap(MODULE, "fd_readdir", |_: i32, _: i32, _: i32, _: i64, _: i32| -> i32 { ERRNO_BADF })?;
  linker.func_wrap(MODULE, "path_filestat_get", |_: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { ERRNO_BADF })?;
  linker.func_wrap(
    MODULE,
    "path_open",
    |_: i32, _: i32, _: i32, _: i32, _: i32, _: i64, _: i64, _: i32, _: i32| -> i32 { ERRNO_BADF },
  )?;
  linker.func_wrap(MODULE, "path_readlink", |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { ERRNO_BADF })?;
  linker.func_wrap(MODULE, "path_unlink_file", |_: i32, _: i32, _: i32| -> i32 { ERRNO_BADF })?;

  Ok(())
}

/// Defines the WASI preview 1 functions the module imports that weren't added
/// by `add_wasi_imports` as functions that fail with "not implemented".
///
/// A toolchain's runtime may import functions that a plugin never ends up
/// calling, so this allows the module to be instantiated without giving it
/// any capabilities. Imports from any other module are left undefined so
/// that instantiating still fails for them.
pub fn add_unsupported_wasi_imports(linker: &mut Linker, store: &mut Store, module: &wasmtime::Module) -> Result<()> {
  for import in module.imports() {
    if import.module() != MODULE {
      continue;
    }
    let ExternType::Func(ty) = import.ty() else {
      continue;
    };
    if linker.get(&mut *store, MODULE, import.name()).is_some() {
      continue;
    }
    let returns_errno = ty.results().len() == 1 && matches!(ty.results().next(), Some(ValType::I32));
    let name = import.name().to_string();
    linker.func_new(MODULE, import.name(), ty, move |_, _, results| {
      if returns_errno {
        results[0] = Val::I32(ERRNO_NOSYS);
        Ok(())
      } else {
        Err(wasmtime::Error::msg(format!("The plugin called the unsupported WASI function {}.", name)))
      }
    })?;
  }
  Ok(())
}

/// Logs the text a plugin writes to stdout or stderr.
pub fn fd_write(mut caller: Caller<'_, WasmHostState>, fd: i32, iovs_ptr: i32, iovs_len: i32, nwritten_ptr: i32) -> i32 {
  if !matches!(fd, 1 | 2) {
    return ERRNO_BADF;
  }

  let Some(memory) = get_memory(&mut caller) else {
    return ERRNO_FAULT;
  };
  let (text, written) = match read_output(memory.data(&caller), iovs_ptr as u32, iovs_len as u32) {
    Ok(output) => output,
    Err(errno) => return errno,
  };
  if !text.is_empty() {
    caller.data().log_output(&text);
  }

  write_memory(&mut caller, nwritten_ptr, &written.to_le_bytes())
}

fn clock_res_get(mut caller: Caller<'_, WasmHostState>, clock_id: i32, resolution_ptr: i32) -> i32 {
  if !matches!(clock_id, CLOCK_REALTIME | CLOCK_MONOTONIC) {
    return ERRNO_INVAL;
  }
  write_memory(&mut caller, resolution_ptr, &CLOCK_RESOLUTION_NANOS.to_le_bytes())
}

fn clock_time_get(mut caller: Caller<'_, WasmHostState>, clock_id: i32, _precision: i64, time_ptr: i32) -> i32 {
  let Some(nanos) = clock_now_nanos(clock_id) else {
    return ERRNO_INVAL;
  };
  let nanos = nanos - nanos % CLOCK_RESOLUTION_NANOS;
  write_memory(&mut caller, time_ptr, &nanos.to_le_bytes())
}

/// Waits for one of the subscriptions to occur, which is how plugins sleep.
fn poll_oneoff(mut caller: Caller<'_, WasmHostState>, subscriptions_ptr: i32, events_ptr: i32, subscriptions_len: i32, events_len_ptr: i32) -> i32 {
  let Some(memory) = get_memory(&mut caller) else {
    return ERRNO_FAULT;
  };
  let data = memory.data(&caller);
  let (events, sleep_duration) = match read_subscriptions(data, subscriptions_ptr as u32, subscriptions_len as u32) {
    Ok(result) => result,
    Err(errno) => return errno,
  };
  let events = events.concat();
  // ensure the events can be written before sleeping
  if get_bytes(data, events_ptr as u32, events.len() as u32).is_none() || get_bytes(data, events_len_ptr as u32, 4).is_none() {
    return ERRNO_FAULT;
  }
  if let Some(duration) = sleep_duration {
    std::thread::sleep(duration);
  }

  let result = write_memory(&mut caller, events_ptr, &events);
  if result != ERRNO_SUCCESS {
    return result;
  }
  write_memory(&mut caller, events_len_ptr, &((events.len() / EVENT_SIZE) as u32).to_le_bytes())
}

fn random_get(mut caller: Caller<'_, WasmHostState>, buf_ptr: i32, buf_len: i32) -> i32 {
  let Some(memory) = get_memory(&mut caller) else {
    return ERRNO_FAULT;
  };
  let start = buf_ptr as u32 as usize;
  let Some(buf) = start
    .checked_add(buf_len as u32 as usize)
    .and_then(|end| memory.data_mut(&mut caller).get_mut(start..end))
  else {
    return ERRNO_FAULT;
  };
  rand::rng().fill_bytes(buf);
  ERRNO_SUCCESS
}

fn proc_exit(_: Caller<'_, WasmHostState>, code: i32) -> wasmtime::Result<()> {
  // exiting the process isn't allowed, so trap instead
  Err(wasmtime::Error::msg(format!("The plugin attempted to exit with code {}.", code)))
}

fn fd_fdstat_get(mut caller: Caller<'_, WasmHostState>, fd: i32, fdstat_ptr: i32) -> i32 {
  let rights = match fd {
    0 => RIGHTS_FD_READ,
    1 | 2 => RIGHTS_FD_WRITE,
    _ => return ERRNO_BADF,
  };
  // filetype: u8, flags: u16, rights_base: u64, rights_inheriting: u64
  let mut fdstat = [0u8; 24];
  fdstat[0] = FILETYPE_CHARACTER_DEVICE;
  fdstat[8..16].copy_from_slice(&rights.to_le_bytes());
  write_memory(&mut caller, fdstat_ptr, &fdstat)
}

fn fd_filestat_get(mut caller: Caller<'_, WasmHostState>, fd: i32, filestat_ptr: i32) -> i32 {
  if !is_std_stream(fd) {
    return ERRNO_BADF;
  }
  // dev: u64, ino: u64, filetype: u8, nlink: u64, size: u64, atim: u64, mtim: u64, ctim: u64
  let mut filestat = [0u8; 64];
  filestat[16] = FILETYPE_CHARACTER_DEVICE;
  write_memory(&mut caller, filestat_ptr, &filestat)
}

fn fd_read(mut caller: Caller<'_, WasmHostState>, fd: i32, _iovs_ptr: i32, _iovs_len: i32, nread_ptr: i32) -> i32 {
  if fd != 0 {
    return ERRNO_BADF;
  }
  // stdin is always at the end of the file
  write_memory(&mut caller, nread_ptr, &0u32.to_le_bytes())
}

fn write_two_zeros(mut caller: Caller<'_, WasmHostState>, first_ptr: i32, second_ptr: i32) -> i32 {
  let result = write_memory(&mut caller, first_ptr, &0u32.to_le_bytes());
  if result != ERRNO_SUCCESS {
    return result;
  }
  write_memory(&mut caller, second_ptr, &0u32.to_le_bytes())
}

fn is_std_stream(fd: i32) -> bool {
  matches!(fd, 0..=2)
}

fn clock_now_nanos(clock_id: i32) -> Option<u64> {
  static MONOTONIC_START: OnceLock<Instant> = OnceLock::new();

  let nanos = match clock_id {
    CLOCK_REALTIME => SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
    CLOCK_MONOTONIC => MONOTONIC_START.get_or_init(Instant::now).elapsed().as_nanos(),
    _ => return None,
  };
  Some(nanos as u64)
}

/// Reads the subscriptions of a poll returning the events that occur along
/// with how long to wait until they do.
///
/// Subscriptions for reading stdin or writing to stdout/stderr are always
/// ready, so only a poll with nothing but clock subscriptions waits.
#[allow(clippy::type_complexity)]
fn read_subscriptions(data: &[u8], subscriptions_ptr: u32, subscriptions_len: u32) -> Result<(Vec<[u8; EVENT_SIZE]>, Option<Duration>), i32> {
  if subscriptions_len == 0 || subscriptions_len > MAX_SUBSCRIPTIONS {
    return Err(ERRNO_INVAL);
  }

  let mut ready_events = Vec::new();
  let mut timeouts = Vec::new();
  for i in 0..subscriptions_len {
    let subscription_ptr = subscriptions_ptr.checked_add(i * SUBSCRIPTION_SIZE).ok_or(ERRNO_FAULT)?;
    // userdata: u64, type: u8, then for a clock (id: u32, timeout: u64, precision: u64, flags: u16)
    // at an offset of 16 bytes and for the others (fd: u32)
    let subscription = get_bytes(data, subscription_ptr, SUBSCRIPTION_SIZE).ok_or(ERRNO_FAULT)?;
    let user_data = &subscription[0..8];
    let event_type = subscription[8];
    let id = u32::from_le_bytes(subscription[16..20].try_into().unwrap());
    match event_type {
      EVENTTYPE_CLOCK => {
        let timeout = u64::from_le_bytes(subscription[24..32].try_into().unwrap());
        let flags = u16::from_le_bytes(subscription[40..42].try_into().unwrap());
        match clock_now_nanos(id as i32) {
          Some(now) if flags & SUBCLOCKFLAGS_ABSTIME != 0 => timeouts.push((user_data, timeout.saturating_sub(now))),
          Some(_) => timeouts.push((user_data, timeout)),
          None => ready_events.push(create_event(user_data, ERRNO_INVAL, event_type)),
        }
      }
      EVENTTYPE_FD_READ | EVENTTYPE_FD_WRITE => {
        let is_valid_fd = if event_type == EVENTTYPE_FD_READ { id == 0 } else { matches!(id, 1 | 2) };
        let errno = if is_valid_fd { ERRNO_SUCCESS } else { ERRNO_BADF };
        ready_events.push(create_event(user_data, errno, event_type));
      }
      _ => return Err(ERRNO_INVAL),
    }
  }

  if !ready_events.is_empty() {
    // the clocks that don't need to wait have also occurred
    let elapsed_timeouts = timeouts.into_iter().filter(|(_, timeout)| *timeout == 0);
    ready_events.extend(elapsed_timeouts.map(|(user_data, _)| create_event(user_data, ERRNO_SUCCESS, EVENTTYPE_CLOCK)));
    return Ok((ready_events, None));
  }
  let timeout = timeouts.iter().map(|(_, timeout)| *timeout).min().unwrap_or(0);
  let events = timeouts
    .into_iter()
    .filter(|(_, event_timeout)| *event_timeout == timeout)
    .map(|(user_data, _)| create_event(user_data, ERRNO_SUCCESS, EVENTTYPE_CLOCK))
    .collect();
  Ok((events, Some(Duration::from_nanos(timeout))))
}

fn create_event(user_data: &[u8], errno: i32, event_type: u8) -> [u8; EVENT_SIZE] {
  // userdata: u64, error: u16, type: u8, fd_readwrite: (nbytes: u64, flags: u16)
  let mut event = [0u8; EVENT_SIZE];
  event[0..8].copy_from_slice(user_data);
  event[8..10].copy_from_slice(&(errno as u16).to_le_bytes());
  event[10] = event_type;
  event
}

/// Reads the text of a write along with the number of bytes that were
/// consumed from the provided buffers.
fn read_output(data: &[u8], iovs_ptr: u32, iovs_len: u32) -> Result<(String, u32), i32> {
  if iovs_len > MAX_IOVS {
    return Err(ERRNO_INVAL);
  }

  let mut bytes = Vec::new();
  for i in 0..iovs_len {
    let iovec_ptr = iovs_ptr.checked_add(i * 8).ok_or(ERRNO_FAULT)?;
    let iovec = get_bytes(data, iovec_ptr, 8).ok_or(ERRNO_FAULT)?;
    let buf_ptr = u32::from_le_bytes(iovec[0..4].try_into().unwrap());
    let buf_len = u32::from_le_bytes(iovec[4..8].try_into().unwrap());
    let buf = get_bytes(data, buf_ptr, buf_len).ok_or(ERRNO_FAULT)?;
    let remaining = MAX_WRITE_BYTES - bytes.len();
    bytes.extend_from_slice(&buf[..buf.len().min(remaining)]);
    if bytes.len() == MAX_WRITE_BYTES {
      break;
    }
  }
  Ok((sanitize_output(&bytes), bytes.len() as u32))
}

/// Replaces the control characters in a plugin's output so that a plugin
/// can't send escape sequences to the terminal.
fn sanitize_output(bytes: &[u8]) -> String {
  String::from_utf8_lossy(bytes)
    .chars()
    .filter(|c| *c != '\r')
    .map(|c| if c.is_control() && !matches!(c, '\n' | '\t') { '\u{FFFD}' } else { c })
    .collect()
}

fn write_memory(caller: &mut Caller<'_, WasmHostState>, ptr: i32, bytes: &[u8]) -> i32 {
  match get_memory(caller) {
    Some(memory) if memory.write(caller, ptr as u32 as usize, bytes).is_ok() => ERRNO_SUCCESS,
    _ => ERRNO_FAULT,
  }
}

fn get_bytes(data: &[u8], ptr: u32, len: u32) -> Option<&[u8]> {
  let start = ptr as usize;
  data.get(start..start.checked_add(len as usize)?)
}

// the memory is looked up from the caller's exports rather than the host
// state because these functions may be called while the module is being
// initialized, which is before the host state has the memory
fn get_memory(caller: &mut Caller<'_, WasmHostState>) -> Option<Memory> {
  match caller.get_export("memory") {
    Some(Extern::Memory(memory)) => Some(memory),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use wasmtime::Engine;
  use wasmtime::Instance;
  use wasmtime::Module;

  use super::*;

  /// The functions that get imported and then exported by a wrapper function
  /// in the test module along with their parameters.
  const FUNCTIONS: &[(&str, &str)] = &[
    ("fd_write", "i32 i32 i32 i32"),
    ("fd_read", "i32 i32 i32 i32"),
    ("fd_close", "i32"),
    ("fd_fdstat_get", "i32 i32"),
    ("fd_filestat_get", "i32 i32"),
    ("fd_prestat_get", "i32 i32"),
    ("clock_time_get", "i32 i64 i32"),
    ("clock_res_get", "i32 i32"),
    ("random_get", "i32 i32"),
    ("poll_oneoff", "i32 i32 i32 i32"),
    ("args_sizes_get", "i32 i32"),
    ("environ_sizes_get", "i32 i32"),
    ("path_open", "i32 i32 i32 i32 i32 i64 i64 i32 i32"),
    ("path_unlink_file", "i32 i32 i32"),
    // not supported
    ("sock_send", "i32 i32 i32 i32 i32"),
    ("path_create_directory", "i32 i32 i32"),
  ];

  #[test]
  fn fails_without_memory() {
    for body in ["", r#"(func (export "memory"))"#, r#"(global (export "memory") i32 (i32.const 5))"#] {
      let mut module = TestModule::new("", body);
      assert_eq!(module.call("clock_time_get", &[i(0), Val::I64(0), i(0)]), ERRNO_FAULT);
      assert_eq!(module.call("random_get", &[i(0), i(16)]), ERRNO_FAULT);
      assert_eq!(module.call("fd_write", &[i(1), i(0), i(1), i(0)]), ERRNO_FAULT);
      assert_eq!(module.call("args_sizes_get", &[i(0), i(4)]), ERRNO_FAULT);
      assert_eq!(module.call("fd_fdstat_get", &[i(1), i(0)]), ERRNO_FAULT);
    }
  }

  #[test]
  fn fails_for_out_of_bounds_pointers() {
    let mut module = TestModule::with_memory();
    for (ptr, len) in [(65530, 10), (-1, -1), (-1, 1), (65536, 1), (0, 65537), (0, -1)] {
      assert_eq!(module.call("random_get", &[i(ptr), i(len)]), ERRNO_FAULT, "{} {}", ptr, len);
    }
    for (ptr, len) in [(65536, 0), (0, 65536), (65535, 1)] {
      assert_eq!(module.call("random_get", &[i(ptr), i(len)]), ERRNO_SUCCESS, "{} {}", ptr, len);
    }
    for ptr in [65529, 65535, 65536, -1, -8, i32::MIN] {
      assert_eq!(module.call("clock_time_get", &[i(1), Val::I64(0), i(ptr)]), ERRNO_FAULT, "{}", ptr);
      assert_eq!(module.call("clock_res_get", &[i(1), i(ptr)]), ERRNO_FAULT, "{}", ptr);
    }
    assert_eq!(module.call("args_sizes_get", &[i(0), i(65534)]), ERRNO_FAULT);
    assert_eq!(module.call("environ_sizes_get", &[i(65534), i(0)]), ERRNO_FAULT);
    assert_eq!(module.call("fd_fdstat_get", &[i(1), i(65520)]), ERRNO_FAULT);
    assert_eq!(module.call("fd_filestat_get", &[i(1), i(65520)]), ERRNO_FAULT);
    assert_eq!(module.call("fd_read", &[i(0), i(0), i(1), i(65534)]), ERRNO_FAULT);
    // nwritten out of bounds
    assert_eq!(module.call("fd_write", &[i(1), i(0), i(0), i(65534)]), ERRNO_FAULT);
    // iovec out of bounds
    assert_eq!(module.call("fd_write", &[i(1), i(65532), i(1), i(0)]), ERRNO_FAULT);
    assert_eq!(module.call("fd_write", &[i(1), i(-8), i(2), i(0)]), ERRNO_FAULT);
  }

  #[test]
  fn clocks() {
    let mut module = TestModule::with_memory();
    assert_eq!(module.call("clock_res_get", &[i(CLOCK_REALTIME), i(0)]), ERRNO_SUCCESS);
    assert_eq!(module.read_u64(0), CLOCK_RESOLUTION_NANOS);
    assert_eq!(module.call("clock_res_get", &[i(2), i(0)]), ERRNO_INVAL);

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
    assert_eq!(module.call("clock_time_get", &[i(CLOCK_REALTIME), Val::I64(0), i(8)]), ERRNO_SUCCESS);
    let time = module.read_u64(8);
    assert!(time >= now - CLOCK_RESOLUTION_NANOS && time < now + 60_000_000_000, "{} {}", time, now);
    assert_eq!(time % CLOCK_RESOLUTION_NANOS, 0);

    assert_eq!(module.call("clock_time_get", &[i(CLOCK_MONOTONIC), Val::I64(0), i(16)]), ERRNO_SUCCESS);
    let first = module.read_u64(16);
    assert_eq!(module.call("clock_time_get", &[i(CLOCK_MONOTONIC), Val::I64(0), i(16)]), ERRNO_SUCCESS);
    assert!(module.read_u64(16) >= first);
    assert_eq!(first % CLOCK_RESOLUTION_NANOS, 0);
    assert_eq!(module.call("clock_time_get", &[i(2), Val::I64(0), i(16)]), ERRNO_INVAL);
  }

  #[test]
  fn poll_oneoff_sleeps() {
    let mut module = TestModule::with_memory();
    // two relative timeouts on the monotonic clock where the second is sooner
    module.write_memory(0, &create_clock_subscription(1, CLOCK_MONOTONIC, 60_000_000_000, 0));
    module.write_memory(48, &create_clock_subscription(2, CLOCK_MONOTONIC, 30_000_000, 0));
    let start = Instant::now();
    assert_eq!(module.call("poll_oneoff", &[i(0), i(200), i(2), i(300)]), ERRNO_SUCCESS);
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(30) && elapsed < Duration::from_secs(30), "{:?}", elapsed);
    let memory = module.memory();
    assert_eq!(&memory[300..304], &1u32.to_le_bytes());
    assert_eq!(&memory[200..232], &create_event(&2u64.to_le_bytes(), ERRNO_SUCCESS, EVENTTYPE_CLOCK));

    // doesn't sleep when the events can't be written
    module.write_memory(0, &create_clock_subscription(1, CLOCK_MONOTONIC, u64::MAX, 0));
    assert_eq!(module.call("poll_oneoff", &[i(0), i(65530), i(1), i(300)]), ERRNO_FAULT);
    assert_eq!(module.call("poll_oneoff", &[i(0), i(200), i(1), i(65534)]), ERRNO_FAULT);
    assert_eq!(module.call("poll_oneoff", &[i(65500), i(200), i(1), i(300)]), ERRNO_FAULT);
    assert_eq!(module.call("poll_oneoff", &[i(0), i(200), i(0), i(300)]), ERRNO_INVAL);
    assert_eq!(module.call("poll_oneoff", &[i(0), i(200), i(-1), i(300)]), ERRNO_INVAL);
  }

  #[test]
  fn read_subscriptions_clocks() {
    // an absolute time in the past doesn't wait
    let data = create_clock_subscription(1, CLOCK_REALTIME, 1, SUBCLOCKFLAGS_ABSTIME);
    let expected_event = create_event(&1u64.to_le_bytes(), ERRNO_SUCCESS, EVENTTYPE_CLOCK);
    assert_eq!(read_subscriptions(&data, 0, 1), Ok((vec![expected_event], Some(Duration::ZERO))));

    // an absolute time in the future waits until then
    let now = clock_now_nanos(CLOCK_REALTIME).unwrap();
    let data = create_clock_subscription(1, CLOCK_REALTIME, now + 60_000_000_000, SUBCLOCKFLAGS_ABSTIME);
    let (events, duration) = read_subscriptions(&data, 0, 1).unwrap();
    assert_eq!(events, vec![expected_event]);
    let duration = duration.unwrap();
    assert!(duration > Duration::from_secs(50) && duration <= Duration::from_secs(60), "{:?}", duration);

    // every subscription with the shortest timeout occurs
    let mut data = create_clock_subscription(1, CLOCK_MONOTONIC, 5, 0).to_vec();
    data.extend(create_clock_subscription(2, CLOCK_MONOTONIC, 10, 0));
    data.extend(create_clock_subscription(3, CLOCK_REALTIME, 5, 0));
    assert_eq!(
      read_subscriptions(&data, 0, 3),
      Ok((
        vec![expected_event, create_event(&3u64.to_le_bytes(), ERRNO_SUCCESS, EVENTTYPE_CLOCK)],
        Some(Duration::from_nanos(5))
      ))
    );

    // unknown clock
    let data = create_clock_subscription(1, 2, 5, 0);
    assert_eq!(
      read_subscriptions(&data, 0, 1),
      Ok((vec![create_event(&1u64.to_le_bytes(), ERRNO_INVAL, EVENTTYPE_CLOCK)], None))
    );
  }

  #[test]
  fn read_subscriptions_standard_streams_always_ready() {
    let mut data = create_clock_subscription(1, CLOCK_MONOTONIC, u64::MAX, 0).to_vec();
    for (user_data, event_type, fd) in [
      (2u64, EVENTTYPE_FD_READ, 0u32),
      (3, EVENTTYPE_FD_WRITE, 1),
      (4, EVENTTYPE_FD_WRITE, 3),
      (5, EVENTTYPE_FD_READ, 1),
    ] {
      let mut subscription = [0u8; 48];
      subscription[0..8].copy_from_slice(&user_data.to_le_bytes());
      subscription[8] = event_type;
      subscription[16..20].copy_from_slice(&fd.to_le_bytes());
      data.extend(subscription);
    }
    assert_eq!(
      read_subscriptions(&data, 0, 5),
      Ok((
        vec![
          create_event(&2u64.to_le_bytes(), ERRNO_SUCCESS, EVENTTYPE_FD_READ),
          create_event(&3u64.to_le_bytes(), ERRNO_SUCCESS, EVENTTYPE_FD_WRITE),
          create_event(&4u64.to_le_bytes(), ERRNO_BADF, EVENTTYPE_FD_WRITE),
          create_event(&5u64.to_le_bytes(), ERRNO_BADF, EVENTTYPE_FD_READ),
        ],
        None
      ))
    );

    // clocks that have already elapsed occur along with the streams
    let mut elapsed_data = create_clock_subscription(6, CLOCK_MONOTONIC, 0, 0).to_vec();
    elapsed_data.extend(create_clock_subscription(7, CLOCK_REALTIME, 1, SUBCLOCKFLAGS_ABSTIME));
    elapsed_data.extend(create_clock_subscription(8, CLOCK_MONOTONIC, 1, 0));
    elapsed_data.extend(&data[48..96]);
    assert_eq!(
      read_subscriptions(&elapsed_data, 0, 4),
      Ok((
        vec![
          create_event(&2u64.to_le_bytes(), ERRNO_SUCCESS, EVENTTYPE_FD_READ),
          create_event(&6u64.to_le_bytes(), ERRNO_SUCCESS, EVENTTYPE_CLOCK),
          create_event(&7u64.to_le_bytes(), ERRNO_SUCCESS, EVENTTYPE_CLOCK),
        ],
        None
      ))
    );

    assert_eq!(read_subscriptions(&data, 0, 6), Err(ERRNO_FAULT));
    assert_eq!(read_subscriptions(&data, u32::MAX - 50, 2), Err(ERRNO_FAULT));
    assert_eq!(read_subscriptions(&data, 0, 0), Err(ERRNO_INVAL));
    assert_eq!(read_subscriptions(&data, 0, MAX_SUBSCRIPTIONS + 1), Err(ERRNO_INVAL));
    // unknown event type
    data[8] = 3;
    assert_eq!(read_subscriptions(&data, 0, 5), Err(ERRNO_INVAL));
  }

  #[test]
  fn random_bytes() {
    let mut module = TestModule::with_memory();
    assert_eq!(module.call("random_get", &[i(8), i(32)]), ERRNO_SUCCESS);
    let memory = module.memory();
    assert_ne!(&memory[8..40], &[0u8; 32]);
    // doesn't write outside the buffer
    assert_eq!(&memory[0..8], &[0u8; 8]);
    assert_eq!(&memory[40..48], &[0u8; 8]);
  }

  #[test]
  fn standard_streams() {
    let mut module = TestModule::with_memory();
    assert_eq!(module.call("fd_fdstat_get", &[i(0), i(0)]), ERRNO_SUCCESS);
    let mut expected = [0u8; 24];
    expected[0] = FILETYPE_CHARACTER_DEVICE;
    expected[8] = RIGHTS_FD_READ as u8;
    assert_eq!(&module.memory()[0..24], &expected);
    assert_eq!(module.call("fd_fdstat_get", &[i(2), i(0)]), ERRNO_SUCCESS);
    expected[8] = RIGHTS_FD_WRITE as u8;
    assert_eq!(&module.memory()[0..24], &expected);
    assert_eq!(module.call("fd_fdstat_get", &[i(3), i(0)]), ERRNO_BADF);

    assert_eq!(module.call("fd_filestat_get", &[i(1), i(100)]), ERRNO_SUCCESS);
    assert_eq!(module.memory()[116], FILETYPE_CHARACTER_DEVICE);
    assert_eq!(module.call("fd_filestat_get", &[i(3), i(100)]), ERRNO_BADF);

    // stdin is empty
    module.write_memory(200, &[1, 1, 1, 1]);
    assert_eq!(module.call("fd_read", &[i(0), i(0), i(1), i(200)]), ERRNO_SUCCESS);
    assert_eq!(&module.memory()[200..204], &[0u8; 4]);
    assert_eq!(module.call("fd_read", &[i(1), i(0), i(1), i(200)]), ERRNO_BADF);

    assert_eq!(module.call("fd_close", &[i(1)]), ERRNO_SUCCESS);
    assert_eq!(module.call("fd_close", &[i(3)]), ERRNO_BADF);
  }

  #[test]
  fn fd_write_only_to_stdout_and_stderr() {
    let mut module = TestModule::new(
      "",
      r#"(memory (export "memory") 1)
         (data (i32.const 0) "\64\00\00\00\05\00\00\00\69\00\00\00\00\00\00\00")
         (data (i32.const 100) "hello")"#,
    );
    for fd in [1, 2] {
      module.write_memory(60, &[0; 4]);
      assert_eq!(module.call("fd_write", &[i(fd), i(0), i(2), i(60)]), ERRNO_SUCCESS);
      assert_eq!(&module.memory()[60..64], &5u32.to_le_bytes());
    }
    for fd in [0, 3, 4, -1, i32::MAX] {
      assert_eq!(module.call("fd_write", &[i(fd), i(0), i(2), i(60)]), ERRNO_BADF, "{}", fd);
    }
    assert_eq!(module.call("fd_write", &[i(1), i(0), i(-1), i(60)]), ERRNO_INVAL);
    assert_eq!(module.call("fd_write", &[i(1), i(0), i(MAX_IOVS as i32 + 1), i(60)]), ERRNO_INVAL);
  }

  #[test]
  fn fd_write_in_start_function() {
    // the start function runs while instantiating
    let mut module = TestModule::new(
      "",
      r#"(memory (export "memory") 1)
         (data (i32.const 0) "\64\00\00\00\05\00\00\00")
         (data (i32.const 100) "hello")
         (func $start (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 60))))
         (start $start)"#,
    );
    assert_eq!(&module.memory()[60..64], &5u32.to_le_bytes());
  }

  #[test]
  fn no_file_system_or_environment() {
    let mut module = TestModule::with_memory();
    module.write_memory(0, &[1; 8]);
    assert_eq!(module.call("args_sizes_get", &[i(0), i(4)]), ERRNO_SUCCESS);
    assert_eq!(&module.memory()[0..8], &[0u8; 8]);
    module.write_memory(0, &[1; 8]);
    assert_eq!(module.call("environ_sizes_get", &[i(0), i(4)]), ERRNO_SUCCESS);
    assert_eq!(&module.memory()[0..8], &[0u8; 8]);

    for fd in 0..10 {
      assert_eq!(module.call("fd_prestat_get", &[i(fd), i(0)]), ERRNO_BADF);
      assert_eq!(
        module.call("path_open", &[i(fd), i(0), i(0), i(1), i(0), Val::I64(-1), Val::I64(-1), i(0), i(100)]),
        ERRNO_BADF
      );
      assert_eq!(module.call("path_unlink_file", &[i(fd), i(0), i(1)]), ERRNO_BADF);
    }
  }

  #[test]
  fn proc_exit_traps() {
    let mut module = TestModule::new(
      r#"(import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))"#,
      r#"(func (export "proc_exit") (param i32) (call $proc_exit (local.get 0)))"#,
    );
    let err = module.try_call("proc_exit", &[i(3)]).unwrap_err();
    assert!(format!("{:#}", err).contains("The plugin attempted to exit with code 3."));
  }

  #[test]
  fn unsupported_wasi_imports() {
    let mut module = TestModule::new(
      r#"(import "wasi_snapshot_preview1" "unknown" (func $unknown (param i32)))"#,
      r#"(func (export "unknown") (param i32) (call $unknown (local.get 0)))"#,
    );
    assert_eq!(module.call("sock_send", &[i(0), i(0), i(0), i(0), i(0)]), ERRNO_NOSYS);
    assert_eq!(module.call("path_create_directory", &[i(3), i(0), i(1)]), ERRNO_NOSYS);
    let err = module.try_call("unknown", &[i(0)]).unwrap_err();
    assert!(format!("{:#}", err).contains("The plugin called the unsupported WASI function unknown."));
  }

  #[test]
  fn other_imports_stay_undefined() {
    for import in [
      r#"(import "dprint" "host_future" (func))"#,
      r#"(import "env" "something" (func))"#,
      r#"(import "wasi_snapshot_preview1" "memory" (memory 1))"#,
      r#"(import "wasi_snapshot_preview1" "global" (global i32))"#,
      // wrong signature for a supported function
      r#"(import "wasi_snapshot_preview1" "fd_advise" (func (param i64) (result i32)))"#,
    ] {
      assert!(TestModule::try_new(import, "").is_err(), "{}", import);
    }
  }

  #[test]
  fn read_output_combines_buffers() {
    let mut data = vec![0u8; 200];
    data[0..16].copy_from_slice(&[100, 0, 0, 0, 3, 0, 0, 0, 110, 0, 0, 0, 2, 0, 0, 0]);
    data[100..103].copy_from_slice(b"hel");
    data[110..112].copy_from_slice(b"lo");
    assert_eq!(read_output(&data, 0, 2), Ok(("hello".to_string(), 5)));
    assert_eq!(read_output(&data, 0, 0), Ok((String::new(), 0)));
    // buffer out of bounds
    data[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(read_output(&data, 0, 2), Err(ERRNO_FAULT));
    assert_eq!(read_output(&data, u32::MAX - 4, 2), Err(ERRNO_FAULT));
    assert_eq!(read_output(&data, 0, MAX_IOVS + 1), Err(ERRNO_INVAL));
  }

  #[test]
  fn read_output_limits_size() {
    let len = MAX_WRITE_BYTES as u32 - 10;
    let mut data = vec![b'a'; MAX_WRITE_BYTES + 100];
    for iovec in data[0..24].chunks_mut(8) {
      iovec[0..4].copy_from_slice(&50u32.to_le_bytes());
      iovec[4..8].copy_from_slice(&len.to_le_bytes());
    }
    let (text, written) = read_output(&data, 0, 3).unwrap();
    assert_eq!(text.len(), MAX_WRITE_BYTES);
    assert_eq!(written, MAX_WRITE_BYTES as u32);
  }

  #[test]
  fn sanitize_output_replaces_control_characters() {
    assert_eq!(sanitize_output(b"a\tb\r\nc\n"), "a\tb\nc\n");
    assert_eq!(
      sanitize_output(b"\x1b[31mred\x1b[0m\x07\x00\x7f"),
      "\u{FFFD}[31mred\u{FFFD}[0m\u{FFFD}\u{FFFD}\u{FFFD}"
    );
    // C1 control characters (ex. the single character CSI and OSC)
    assert_eq!(sanitize_output("\u{9b}31m\u{9d}".as_bytes()), "\u{FFFD}31m\u{FFFD}");
    assert_eq!(sanitize_output(b"hi\xff"), "hi\u{FFFD}");
  }

  struct TestModule {
    store: Store,
    instance: Instance,
  }

  impl TestModule {
    pub fn new(imports: &str, body: &str) -> Self {
      Self::try_new(imports, body).unwrap()
    }

    pub fn with_memory() -> Self {
      Self::new("", r#"(memory (export "memory") 1)"#)
    }

    /// Creates a module that exports functions calling the WASI functions.
    pub fn try_new(imports: &str, body: &str) -> Result<Self> {
      let engine = Engine::default();
      let mut wat = format!("(module {}", imports);
      for (name, params) in FUNCTIONS {
        wat.push_str(&format!(r#"(import "{}" "{}" (func ${} (param {}) (result i32)))"#, MODULE, name, name, params));
      }
      for (name, params) in FUNCTIONS {
        let args = (0..params.split(' ').count()).map(|i| format!("(local.get {})", i)).collect::<String>();
        wat.push_str(&format!(
          r#"(func (export "{}") (param {}) (result i32) (call ${} {}))"#,
          name, params, name, args
        ));
      }
      wat.push_str(body);
      wat.push(')');
      let wasm = wat::parse_str(wat)?;
      let module = Module::new(&engine, wasm)?;
      let mut store = Store::new(&engine, WasmHostState::Empty);
      let mut linker = Linker::new(&engine);
      add_wasi_imports(&mut linker)?;
      add_unsupported_wasi_imports(&mut linker, &mut store, &module)?;
      let instance = linker.instantiate(&mut store, &module)?;
      Ok(Self { store, instance })
    }

    pub fn call(&mut self, name: &str, args: &[Val]) -> i32 {
      self.try_call(name, args).unwrap()
    }

    pub fn try_call(&mut self, name: &str, args: &[Val]) -> Result<i32> {
      let func = self.instance.get_func(&mut self.store, name).unwrap();
      let mut results = vec![Val::I32(0); func.ty(&self.store).results().len()];
      func.call(&mut self.store, args, &mut results)?;
      Ok(results.first().map(|v| v.unwrap_i32()).unwrap_or(0))
    }

    pub fn memory(&mut self) -> Vec<u8> {
      self.instance.get_memory(&mut self.store, "memory").unwrap().data(&self.store).to_vec()
    }

    pub fn read_u64(&mut self, ptr: usize) -> u64 {
      u64::from_le_bytes(self.memory()[ptr..ptr + 8].try_into().unwrap())
    }

    pub fn write_memory(&mut self, ptr: usize, bytes: &[u8]) {
      let memory = self.instance.get_memory(&mut self.store, "memory").unwrap();
      memory.write(&mut self.store, ptr, bytes).unwrap();
    }
  }

  fn create_clock_subscription(user_data: u64, clock_id: i32, timeout: u64, flags: u16) -> [u8; 48] {
    let mut subscription = [0u8; 48];
    subscription[0..8].copy_from_slice(&user_data.to_le_bytes());
    subscription[8] = EVENTTYPE_CLOCK;
    subscription[16..20].copy_from_slice(&clock_id.to_le_bytes());
    subscription[24..32].copy_from_slice(&timeout.to_le_bytes());
    subscription[40..42].copy_from_slice(&flags.to_le_bytes());
    subscription
  }

  fn i(value: i32) -> Val {
    Val::I32(value)
  }
}
