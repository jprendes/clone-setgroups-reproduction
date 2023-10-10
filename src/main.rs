use std::{fs::File, num::NonZeroUsize};

use nix::{
    sys::mman,
    unistd::{Gid, Pid},
};

fn clone(cb: impl FnOnce() -> i32) -> Result<Pid, nix::Error> {
    const STACK_SIZE: usize = 500 * 1024; // 500K
    const PAGE_SIZE: usize = 4 * 1024; // 4K

    let child_stack = unsafe {
        // Since nix = "0.27.1", `mmap()` requires a generic type `F: AsFd`.
        // `::<File>` doesn't have any meaning because we won't use it.
        mman::mmap::<File>(
            None,
            NonZeroUsize::new(STACK_SIZE).unwrap(),
            mman::ProtFlags::PROT_READ | mman::ProtFlags::PROT_WRITE,
            mman::MapFlags::MAP_PRIVATE | mman::MapFlags::MAP_ANONYMOUS | mman::MapFlags::MAP_STACK,
            None,
            0,
        )?
    };

    unsafe {
        mman::mprotect(child_stack, PAGE_SIZE, mman::ProtFlags::PROT_NONE)?;
    };

    let child_stack_top = unsafe { child_stack.add(STACK_SIZE) };
    let combined_flags = libc::SIGCHLD;

    let cb: Box<dyn FnOnce() -> i32> = Box::new(cb);
    let data = Box::into_raw(Box::new(cb));
    extern "C" fn main(data: *mut libc::c_void) -> libc::c_int {
        unsafe { Box::from_raw(data as *mut Box<dyn FnOnce() -> i32>)() }
    }

    let ret = unsafe {
        libc::clone(
            main,
            child_stack_top,
            combined_flags,
            data as *mut libc::c_void,
        )
    };

    unsafe { drop(Box::from_raw(data)) };

    match ret {
        -1 => Err(nix::Error::last()),
        pid if ret > 0 => Ok(Pid::from_raw(pid)),
        _ => unreachable!("clone returned a negative pid {ret}"),
    }
}

fn main() {
    let mut handles = vec![];
    for _ in 0..100 {
        let h = std::thread::spawn(|| {
            let pid = clone(|| {
                println!("hello from child process");
                nix::unistd::setgroups(&[Gid::from_raw(0)]).unwrap();
                println!("bye from child process");
                0
            })
            .unwrap();
            let status = nix::sys::wait::waitpid(pid, None).unwrap();
            println!("child finished with status {status:?}");
        });
        handles.push(h);
    }

    for h in handles.into_iter() {
        let _ = h.join();
    }
}
