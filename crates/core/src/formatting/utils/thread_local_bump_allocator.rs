use std::cell::UnsafeCell;
use bumpalo::Bump;

thread_local! {
    static THREAD_LOCAL_BUMP_ALLOCATOR: UnsafeCell<Bump> = UnsafeCell::new(Bump::new());
}

pub fn with_bump_allocator<TReturn>(action: impl FnOnce(&Bump) -> TReturn) -> TReturn {
    THREAD_LOCAL_BUMP_ALLOCATOR.with(|bump_cell| {
        unsafe {
            let bump = bump_cell.get();
            action(&*bump)
        }
    })
}

pub fn with_bump_allocator_mut<TReturn>(action: impl FnMut(&mut Bump) -> TReturn) -> TReturn {
    let mut action = action;
    THREAD_LOCAL_BUMP_ALLOCATOR.with(|bump_cell| {
        unsafe {
            let bump = bump_cell.get();
            action(&mut *bump)
        }
    })
}