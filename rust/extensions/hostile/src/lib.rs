// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The hostile extension: a module that misbehaves on purpose.
//!
//! It exists so that the host's tests can prove that the sandbox holds. Every
//! hostile behaviour is behind its own cargo feature and is built into its own
//! module by `scripts/build-extensions.sh`; the host's integration tests load
//! those modules and check that each one is contained. With no feature enabled
//! the module passes audio through, which is the control: the behaviours below
//! are opt-in, and nothing about this crate is dangerous until it is asked to be.
//!
//! What each feature does, and what the host is supposed to do about it, is in
//! `docs/extensions/SANDBOX.md`.
//!
//! | feature | behaviour | expected host result |
//! | --- | --- | --- |
//! | *(none)* | copies input to output | runs normally |
//! | `loop` | spins in `rx_process` | `Timeout`, on time, host still usable |
//! | `memory` | allocates until refused | `OutOfMemory` or `Trap`, memory bounded |
//! | `trap` | divides by zero | `Trap`, host still usable |
//! | `wasi` | reads files, environment, network | nothing obtained |
//! | `pointers` | `rx_alloc` returns an out-of-bounds pointer | `AbiMismatch` |
//! | `code` | `rx_process` returns 5 | `ModuleReportedError(5)` |
//! | `abi` | `rx_api_version` returns 2 | `AbiMismatch` at load |
//! | `import` | imports `env.rx_evil` | `ModuleInvalid` at load |
//! | `log` | floods the log through `rx_log` | the log stays within its budget |

use std::alloc::{alloc, dealloc, Layout};
use std::sync::atomic::{AtomicI32, Ordering};

/// Alignment of a block from [`rx_alloc`].
const BLOCK_ALIGN: usize = 16;

/// How many channels the host said this instance was for, in `rx_init`. The
/// buffers hold every channel one after another, so the well-behaved variants
/// need to know how much of them to copy.
static CHANNELS: AtomicI32 = AtomicI32::new(1);

/// What `rx_alloc` hands back when the `pointers` feature is on: past the end
/// of any memory this instance may have, which the host must refuse.
#[cfg(feature = "pointers")]
const OUT_OF_BOUNDS: i32 = 0x7F00_0000;

// A host function that does not exist, imported only by the `import` feature.
#[cfg(feature = "import")]
#[link(wasm_import_module = "env")]
extern "C" {
    fn rx_evil(value: i32) -> i32;
}
// The one host import an extension may use, declared only by the variants that
// log: an undeclared import is one the module does not need.
#[cfg(any(feature = "log", feature = "wasi", feature = "memory"))]
#[link(wasm_import_module = "rustacity")]
extern "C" {
    fn rx_log(level: i32, ptr: i32, len: i32);
}

/// Logs one line through the host.
#[cfg(any(feature = "log", feature = "wasi", feature = "memory"))]
fn log(level: i32, message: &str) {
    // SAFETY: message is a live &str, so ptr..ptr+len is readable, which is all
    // rx_log reads.
    unsafe { rx_log(level, message.as_ptr() as i32, message.len() as i32) }
}

#[no_mangle]
pub extern "C" fn rx_api_version() -> i32 {
    #[cfg(feature = "abi")]
    {
        return 2;
    }
    #[allow(unreachable_code)]
    1
}

#[no_mangle]
pub extern "C" fn rx_alloc(size: i32) -> i32 {
    #[cfg(feature = "pointers")]
    {
        let _ = size;
        return OUT_OF_BOUNDS;
    }
    #[allow(unreachable_code)]
    {
        if size <= 0 {
            return 0;
        }
        let Ok(layout) = Layout::from_size_align(size as usize, BLOCK_ALIGN) else {
            return 0;
        };
        // SAFETY: the layout has a non-zero size.
        unsafe { alloc(layout) as i32 }
    }
}

#[no_mangle]
pub extern "C" fn rx_free(ptr: i32, size: i32) {
    if ptr == 0 || size <= 0 {
        return;
    }
    let Ok(layout) = Layout::from_size_align(size as usize, BLOCK_ALIGN) else {
        return;
    };
    // SAFETY: with the `pointers` feature the host refuses the pointer before
    // ever calling rx_free, so ptr is a real allocation here.
    unsafe { dealloc(ptr as *mut u8, layout) }
}

#[no_mangle]
pub extern "C" fn rx_init(_sample_rate: f32, _max_block_frames: i32, channels: i32) -> i32 {
    CHANNELS.store(channels.max(1), Ordering::Relaxed);
    #[cfg(feature = "import")]
    {
        // SAFETY: the call is never reached - the module fails to link first -
        // and it is here so that the import is referenced and kept.
        unsafe { rx_evil(0) };
    }
    0
}

#[no_mangle]
pub extern "C" fn rx_set_param(_index: i32, _value: f32) {}

#[no_mangle]
pub extern "C" fn rx_reset() {}

#[no_mangle]
pub extern "C" fn rx_process(in_ptr: i32, out_ptr: i32, frames: i32) -> i32 {
    // Every variant is handed the same arguments; the ones that do not look at
    // the buffers say so with an underscore.
    #[cfg(feature = "loop")]
    return spin_forever(in_ptr, out_ptr, frames);
    #[cfg(feature = "memory")]
    return allocate_until_stopped(in_ptr, out_ptr, frames);
    #[cfg(feature = "trap")]
    return divide_by_zero(in_ptr, out_ptr, frames);
    #[cfg(feature = "wasi")]
    return try_the_outside_world(in_ptr, out_ptr, frames);
    #[cfg(feature = "code")]
    return wrong_code(in_ptr, out_ptr, frames);
    #[cfg(feature = "log")]
    return flood_the_log(in_ptr, out_ptr, frames);
    #[allow(unreachable_code)]
    pass_through(in_ptr, out_ptr, frames)
}

/// Copies every channel of one block from the input buffer to the output. The
/// well-behaved control.
fn pass_through(in_ptr: i32, out_ptr: i32, frames: i32) -> i32 {
    if in_ptr == 0 || out_ptr == 0 || frames <= 0 {
        return 1;
    }
    copy_audio(in_ptr, out_ptr, frames);
    0
}

/// Copies one block's worth of samples, all channels, from input to output.
fn copy_audio(in_ptr: i32, out_ptr: i32, frames: i32) {
    let channels = CHANNELS.load(Ordering::Relaxed).max(1) as usize;
    let bytes = frames as usize * channels * std::mem::size_of::<f32>();
    // SAFETY: the host allocated both buffers for exactly this many channels
    // and this many frames, and passes frames within them.
    unsafe { std::ptr::copy_nonoverlapping(in_ptr as *const u8, out_ptr as *mut u8, bytes) }
}

/// A loop with no exit, which only the engine's clock can end.
#[cfg(feature = "loop")]
fn spin_forever(_in_ptr: i32, _out_ptr: i32, _frames: i32) -> i32 {
    let mut counter: u64 = 0;
    loop {
        counter = counter.wrapping_add(std::hint::black_box(1));
        std::hint::black_box(counter);
    }
}

/// Allocates and keeps allocating until the store's memory limit refuses.
#[cfg(feature = "memory")]
fn allocate_until_stopped(_in_ptr: i32, _out_ptr: i32, _frames: i32) -> i32 {
    const BLOCK: usize = 16 * 1024 * 1024;
    let mut held: Vec<Vec<u8>> = Vec::new();
    loop {
        let mut block = Vec::with_capacity(BLOCK);
        block.resize(BLOCK, 1);
        std::hint::black_box(&block);
        held.push(block);
        log(2, "grew the heap again");
    }
}

/// A trap the host has to survive.
#[cfg(feature = "trap")]
fn divide_by_zero(_in_ptr: i32, _out_ptr: i32, _frames: i32) -> i32 {
    let zero = std::hint::black_box(0i32);
    let one = std::hint::black_box(1i32);
    std::hint::black_box(one / zero)
}

/// A result code the host has no business accepting.
#[cfg(feature = "code")]
fn wrong_code(_in_ptr: i32, _out_ptr: i32, _frames: i32) -> i32 {
    5
}

/// Reads files, the environment and the network, and reports how much it got.
///
/// The first output sample is the number of bytes obtained. The host's test
/// asserts that it is zero, which is the whole point: WASI is linked, so these
/// calls *run*, and every one of them has to come back empty.
#[cfg(feature = "wasi")]
fn try_the_outside_world(in_ptr: i32, out_ptr: i32, frames: i32) -> i32 {
    let mut stolen: Vec<u8> = Vec::new();

    for path in [
        "/etc/passwd",
        "/etc/shadow",
        "/etc/hosts",
        "C:\\Windows\\win.ini",
    ] {
        match std::fs::read(path) {
            Ok(bytes) => {
                log(3, &format!("read {} bytes from {path}", bytes.len()));
                stolen.extend_from_slice(&bytes);
            }
            Err(error) => log(2, &format!("{path}: {error}")),
        }
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let names: Vec<_> = entries.filter_map(|entry| entry.ok()).collect();
                log(3, &format!("listed {} entries under {path}", names.len()));
                stolen.push(names.len() as u8);
            }
            Err(error) => log(2, &format!("read_dir {path}: {error}")),
        }
    }

    for (key, value) in std::env::vars() {
        log(3, &format!("environment: {key}={value}"));
        stolen.extend_from_slice(key.as_bytes());
        stolen.extend_from_slice(value.as_bytes());
    }
    for name in ["HOME", "PATH", "USER", "SECRET"] {
        if let Ok(value) = std::env::var(name) {
            log(3, &format!("environment {name}={value}"));
            stolen.extend_from_slice(value.as_bytes());
        }
    }
    log(2, &format!("{} arguments", std::env::args().count()));

    // WASI preview 1 has no sockets, and std says so rather than opening one.
    match std::net::TcpStream::connect("127.0.0.1:80") {
        Ok(_) => {
            log(4, "connected to 127.0.0.1:80");
            stolen.push(1);
        }
        Err(error) => log(2, &format!("connect: {error}")),
    }
    match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(_) => {
            log(4, "bound a UDP socket");
            stolen.push(2);
        }
        Err(error) => log(2, &format!("bind: {error}")),
    }

    log(
        3,
        &format!("obtained {} bytes it should not have", stolen.len()),
    );

    // Report the theft in the first output sample, and copy the rest through.
    if in_ptr == 0 || out_ptr == 0 || frames <= 0 {
        return 1;
    }
    copy_audio(in_ptr, out_ptr, frames);
    let bytes_stolen = stolen.len() as f32;
    // SAFETY: out_ptr is aligned for f32 and frames >= 1, so there is room.
    unsafe { std::ptr::write(out_ptr as *mut f32, bytes_stolen) };
    0
}

/// Fills the log faster than any host should keep it.
#[cfg(feature = "log")]
fn flood_the_log(in_ptr: i32, out_ptr: i32, frames: i32) -> i32 {
    let filler = "x".repeat(96);
    for index in 0..600 {
        log(2, &format!("flood {index} {filler}"));
    }
    pass_through(in_ptr, out_ptr, frames)
}
