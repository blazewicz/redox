#![crate_type="staticlib"]
#![feature(alloc)]
#![feature(allocator)]
#![feature(asm)]
#![feature(box_syntax)]
#![feature(collections)]
#![feature(core_intrinsics)]
#![feature(core_simd)]
#![feature(core_str_ext)]
#![feature(core_slice_ext)]
#![feature(fnbox)]
#![feature(fundamental)]
#![feature(lang_items)]
#![feature(no_std)]
#![feature(unboxed_closures)]
#![feature(unsafe_no_drop_flag)]
#![feature(unwind_attributes)]
#![feature(vec_push_all)]
#![feature(raw)]
#![feature(slice_concat_ext)]
#![no_std]

#[macro_use]
extern crate alloc;

#[macro_use]
extern crate collections;

use alloc::boxed::Box;

use collections::string::{String, ToString};
use collections::vec::Vec;

use core::{mem, ptr};
use core::slice::{self, SliceExt};
use core::str;
use core::raw::Repr;

use scheduler::context::*;
use common::debug;
use common::event::{self, Event, EventOption};
use common::memory;
use common::paging::Page;
use common::queue::Queue;
use schemes::Url;
use common::time::Duration;

use drivers::pci::*;
use drivers::pio::*;
use drivers::ps2::*;
use drivers::rtc::*;
use drivers::serial::*;

pub use externs::*;

use graphics::bmp::BmpFile;
use graphics::display::{self, Display};
use graphics::point::Point;

use programs::package::*;
use programs::scheme::*;
use programs::session::*;

use schemes::arp::*;
use schemes::context::*;
use schemes::debug::*;
use schemes::ethernet::*;
use schemes::icmp::*;
use schemes::ip::*;
use schemes::memory::*;
use schemes::random::*;
use schemes::time::*;
use schemes::window::*;
use schemes::display::*;

use syscall::common::Regs;
use syscall::handle::*;

/// Allocation
pub mod alloc_system;
/// Audio
pub mod audio;
/// Common std-like functionality
#[macro_use]
pub mod common;
/// Various drivers
pub mod drivers;
/// Externs
pub mod externs;
/// Various graphical methods
pub mod graphics;
/// Network
pub mod network;
/// Panic
pub mod panic;
/// Programs
pub mod programs;
/// Schemes
pub mod schemes;
/// Scheduling
pub mod scheduler;
/// System calls
pub mod syscall;
/// USB input/output
pub mod usb;

/// Default display for debugging
static mut debug_display: *mut Display = 0 as *mut Display;
/// Default point for debugging
static mut debug_point: Point = Point { x: 0, y: 0 };
/// Draw debug
static mut debug_draw: bool = false;
/// Redraw debug
static mut debug_redraw: bool = false;
/// Debug command
static mut debug_command: *mut String = 0 as *mut String;

/// Clock realtime (default)
static mut clock_realtime: Duration = Duration {
    secs: 0,
    nanos: 0
};

/// Monotonic clock
static mut clock_monotonic: Duration = Duration {
    secs: 0,
    nanos: 0
};

/// Pit duration
static PIT_DURATION: Duration = Duration {
    secs: 0,
    nanos: 2250286
};

/// Session pointer
static mut session_ptr: *mut Session = 0 as *mut Session;

/// Event pointer
static mut events_ptr: *mut Queue<Event> = 0 as *mut Queue<Event>;

/// Bounded slice abstraction
///
/// # Code Migration
///
/// `foo[a..b]` => `foo.get_slice(Some(a), Some(b))`
///
/// `foo[a..]` => `foo.get_slice(Some(a), None)`
///
/// `foo[..b]` => `foo.get_slice(None, Some(b))`
///
pub trait GetSlice { fn get_slice(&self, a: Option<usize>, b: Option<usize>) -> &Self; }

impl GetSlice for str {
    fn get_slice(&self, a: Option<usize>, b: Option<usize>) -> &Self {
        let slice = unsafe { slice::from_raw_parts(self.repr().data, self.repr().len) };
        let a = if let Some(tmp) = a {
            let len = slice.len();
            if tmp > len { len }
            else { tmp }
        } else {
            0
        };
        let b = if let Some(tmp) = b {
            let len = slice.len();
            if tmp > len { len }
            else { tmp }
        } else {
            slice.len()
        };

        if a >= b { return ""; }

        unsafe { str::from_utf8_unchecked(&slice[a..b]) }
    }
}

impl<T> GetSlice for [T] {
    fn get_slice(&self, a: Option<usize>, b: Option<usize>) -> &Self {
        let slice = unsafe { slice::from_raw_parts(SliceExt::as_ptr(self), SliceExt::len(self)) };
        let a = if let Some(tmp) = a {
            let len = slice.len();
            if tmp > len { len }
            else { tmp }
        } else {
            0
        };
        let b = if let Some(tmp) = b {
            let len = slice.len();
            if tmp > len { len }
            else { tmp }
        } else {
            slice.len()
        };

        if a >= b { return &[]; }

        &slice[a..b]
    }
}

/// Idle loop (active while idle)
unsafe fn idle_loop() -> ! {
    loop {
        asm!("cli");

        let mut halt = true;

        let contexts = & *contexts_ptr;
        for i in 1..contexts.len() {
            match contexts.get(i) {
                Some(context) => if context.interrupted {
                    halt = false;
                    break;
                },
                None => ()
            }
        }

        if halt {
            asm!("sti");
            asm!("hlt");
        } else {
            asm!("sti");
        }

        context_switch(false);
    }
}

/// Event poll loop
unsafe fn poll_loop() -> ! {
    let session = &mut *session_ptr;

    loop {
        session.on_poll();

        context_switch(false);
    }
}

/// Event loop
unsafe fn event_loop() -> ! {
    let session = &mut *session_ptr;
    let events = &mut *events_ptr;
    let mut cmd = String::new();
    loop {
        loop {
            let reenable = scheduler::start_no_ints();

            let event_option = events.pop();

            scheduler::end_no_ints(reenable);

            match event_option {
                Some(event) => {
                    if debug_draw {
                        match event.to_option() {
                            EventOption::Key(key_event) => {
                                if key_event.pressed {
                                    match key_event.scancode {
                                        event::K_F2 => {
                                            ::debug_draw = false;
                                            (*::session_ptr).redraw = true;
                                        },
                                        event::K_BKSP => if !cmd.is_empty() {
                                            debug::db(8);
                                            cmd.pop();
                                        },
                                        _ => match key_event.character {
                                            '\0' => (),
                                            '\n' => {
                                                let reenable = scheduler::start_no_ints();
                                                *::debug_command = cmd.clone() + "\n";
                                                scheduler::end_no_ints(reenable);

                                                cmd.clear();
                                                debug::dl();
                                            },
                                            _ => {
                                                cmd.push(key_event.character);
                                                debug::dc(key_event.character);
                                            },
                                        },
                                    }
                                }
                            },
                            _ => (),
                        }
                    } else {
                        if event.code == 'k' && event.b as u8 == event::K_F1 && event.c > 0 {
                            ::debug_draw = true;
                            ::debug_redraw = true;
                        } else {
                            session.event(event);
                        }
                    }
                },
                None => break
            }
        }

        if debug_draw {
            let display = &*debug_display;
            if debug_redraw {
                debug_redraw = false;
                display.flip();
            }
        } else {
            session.redraw();
        }

        context_switch(false);
    }
}

/// Initialize debug
pub unsafe fn debug_init() {
    Pio8::new(0x3F8 + 1).write(0x00);
    Pio8::new(0x3F8 + 3).write(0x80);
    Pio8::new(0x3F8 + 0).write(0x03);
    Pio8::new(0x3F8 + 1).write(0x00);
    Pio8::new(0x3F8 + 3).write(0x03);
    Pio8::new(0x3F8 + 2).write(0xC7);
    Pio8::new(0x3F8 + 4).write(0x0B);
    Pio8::new(0x3F8 + 1).write(0x01);
}

/// Initialize kernel
unsafe fn init(font_data: usize) {
    scheduler::start_no_ints();

    debug_display = 0 as *mut Display;
    debug_point = Point { x: 0, y: 0 };
    debug_draw = false;
    debug_redraw = false;

    clock_realtime.secs = 0;
    clock_realtime.nanos = 0;

    clock_monotonic.secs = 0;
    clock_monotonic.nanos = 0;

    contexts_ptr = 0 as *mut Vec<Box<Context>>;
    context_i = 0;
    context_enabled = false;

    session_ptr = 0 as *mut Session;

    events_ptr = 0 as *mut Queue<Event>;

    debug_init();

    Page::init();
    memory::cluster_init();
    //Unmap first page to catch null pointer errors (after reading memory map)
    Page::new(0).unmap();

    ptr::write(display::FONTS, font_data);

    debug_display = Box::into_raw(Display::root());

    debug_draw = true;

    debug_command = Box::into_raw(box String::new());

    debug::d("Redox ");
    debug::dd(mem::size_of::<usize>() * 8);
    debug::d(" bits ");
    debug::dl();

    clock_realtime = Rtc::new().time();

    contexts_ptr = Box::into_raw(box Vec::new());
    (*contexts_ptr).push(Context::root());

    session_ptr = Box::into_raw(Session::new());

    events_ptr = Box::into_raw(box Queue::new());

    let session = &mut *session_ptr;

    session.items.push(Ps2::new());
    session.items.push(Serial::new(0x3F8, 0x4));

    pci_init(session);

    session.items.push(box ContextScheme);
    session.items.push(box DebugScheme);
    session.items.push(box MemoryScheme);
    session.items.push(box RandomScheme);
    session.items.push(box TimeScheme);

    session.items.push(box EthernetScheme);
    session.items.push(box ArpScheme);
    session.items.push(box IcmpScheme);
    session.items.push(box IpScheme {
        arp: Vec::new()
    });
    session.items.push(box DisplayScheme);
    session.items.push(box WindowScheme);

    Context::spawn(box move || {
        poll_loop();
    });
    Context::spawn(box move || {
        event_loop();
    });
    Context::spawn(box move || {
        ArpScheme::reply_loop();
    });
    Context::spawn(box move || {
        IcmpScheme::reply_loop();
    });

    debug::d("Reenabling interrupts\n");

    //Start interrupts
    scheduler::end_no_ints(true);

    //Load cursor before getting out of debug mode
    debug::d("Loading cursor\n");
    if let Some(mut resource) = Url::from_str("file:///ui/cursor.bmp").open() {
        let mut vec: Vec<u8> = Vec::new();
        resource.read_to_end(&mut vec);

        let cursor = BmpFile::from_data(&vec);

        let reenable = scheduler::start_no_ints();
        session.cursor = cursor;
        session.redraw = true;
        scheduler::end_no_ints(reenable);
    }

    debug::d("Loading schemes\n");
    if let Some(mut resource) = Url::from_str("file:///schemes/").open() {
        let mut vec: Vec<u8> = Vec::new();
        resource.read_to_end(&mut vec);

        for folder in String::from_utf8_unchecked(vec).lines() {
            if folder.ends_with('/') {
                let scheme_item = SchemeItem::from_url(&Url::from_string("file:///schemes/".to_string() + &folder));

                let reenable = scheduler::start_no_ints();
                session.items.push(scheme_item);
                scheduler::end_no_ints(reenable);
            }
        }
    }

    debug::d("Loading apps\n");
    if let Some(mut resource) = Url::from_str("file:///apps/").open() {
        let mut vec: Vec<u8> = Vec::new();
        resource.read_to_end(&mut vec);

        for folder in String::from_utf8_unchecked(vec).lines() {
            if folder.ends_with('/') {
                let package = Package::from_url(&Url::from_string("file:///apps/".to_string() + folder));

                let reenable = scheduler::start_no_ints();
                session.packages.push(package);
                session.redraw = true;
                scheduler::end_no_ints(reenable);
            }
        }
    }

    debug::d("Loading background\n");
    if let Some(mut resource) = Url::from_str("file:///ui/background.bmp").open() {
        let mut vec: Vec<u8> = Vec::new();
        if resource.read_to_end(&mut vec).is_some() {
            debug::d("Read background\n");
        } else {
            debug::d("Failed to read background at: ");
            debug::d(Url::from_str("file:///ui/background.bmp").reference());
            debug::d("\n");
        }

        let background = BmpFile::from_data(&vec);

        let reenable = scheduler::start_no_ints();
        session.background = background;
        session.redraw = true;
        scheduler::end_no_ints(reenable);
    }

    debug::d("Enabling context switching\n");
    debug_draw = false;
    context_enabled = true;
}

fn dr(reg: &str, value: usize) {
    debug::d(reg);
    debug::d(": ");
    debug::dh(value as usize);
    debug::dl();
}

#[cold]
#[inline(never)]
#[no_mangle]
/// Take regs for kernel calls and exceptions
pub unsafe extern "cdecl" fn kernel(interrupt: usize, mut regs: &mut Regs) {
    macro_rules! exception {
        ($name:expr) => ({
            debug::d($name);
            debug::dl();

            dr("INT", interrupt);
            dr("CONTEXT", context_i);
            dr("IP", regs.ip);
            dr("FLAGS", regs.flags);
            dr("AX", regs.ax);
            dr("BX", regs.bx);
            dr("CX", regs.cx);
            dr("DX", regs.dx);
            dr("DI", regs.di);
            dr("SI", regs.si);
            dr("BP", regs.bp);
            dr("SP", regs.sp);

            let cr0: usize;
            asm!("mov $0, cr0" : "=r"(cr0) : : : "intel", "volatile");
            dr("CR0", cr0);

            let cr2: usize;
            asm!("mov $0, cr2" : "=r"(cr2) : : : "intel", "volatile");
            dr("CR2", cr2);

            let cr3: usize;
            asm!("mov $0, cr3" : "=r"(cr3) : : : "intel", "volatile");
            dr("CR3", cr3);

            let cr4: usize;
            asm!("mov $0, cr4" : "=r"(cr4) : : : "intel", "volatile");
            dr("CR4", cr4);

            do_sys_exit(-1);
            loop {
                asm!("cli");
                asm!("hlt");
            }
        })
    };

    macro_rules! exception_error {
        ($name:expr) => ({
            debug::d($name);
            debug::dl();

            dr("INT", interrupt);
            dr("CONTEXT", context_i);
            dr("IP", regs.flags);
            dr("FLAGS", regs.error);
            dr("ERROR", regs.ip);
            dr("AX", regs.ax);
            dr("BX", regs.bx);
            dr("CX", regs.cx);
            dr("DX", regs.dx);
            dr("DI", regs.di);
            dr("SI", regs.si);
            dr("BP", regs.bp);
            dr("SP", regs.sp);

            let cr0: usize;
            asm!("mov $0, cr0" : "=r"(cr0) : : : "intel", "volatile");
            dr("CR0", cr0);

            let cr2: usize;
            asm!("mov $0, cr2" : "=r"(cr2) : : : "intel", "volatile");
            dr("CR2", cr2);

            let cr3: usize;
            asm!("mov $0, cr3" : "=r"(cr3) : : : "intel", "volatile");
            dr("CR3", cr3);

            let cr4: usize;
            asm!("mov $0, cr4" : "=r"(cr4) : : : "intel", "volatile");
            dr("CR4", cr4);

            do_sys_exit(-1);
            loop {
                asm!("cli");
                asm!("hlt");
            }
        })
    };

    if interrupt >= 0x20 && interrupt < 0x30 {
        if interrupt >= 0x28 {
            Pio8::new(0xA0).write(0x20);
        }

        Pio8::new(0x20).write(0x20);
    }

    match interrupt {
        0x20 => {
            let reenable = scheduler::start_no_ints();
            clock_realtime = clock_realtime + PIT_DURATION;
            clock_monotonic = clock_monotonic + PIT_DURATION;
            scheduler::end_no_ints(reenable);

            context_switch(true);
        }
        0x21 => (*session_ptr).on_irq(0x1), // keyboard
        0x23 => (*session_ptr).on_irq(0x3), // serial 2 and 4
        0x24 => (*session_ptr).on_irq(0x4), // serial 1 and 3
        0x25 => (*session_ptr).on_irq(0x5), //parallel 2
        0x26 => (*session_ptr).on_irq(0x6), //floppy
        0x27 => (*session_ptr).on_irq(0x7), //parallel 1 or spurious
        0x28 => (*session_ptr).on_irq(0x8), //RTC
        0x29 => (*session_ptr).on_irq(0x9), //pci
        0x2A => (*session_ptr).on_irq(0xA), //pci
        0x2B => (*session_ptr).on_irq(0xB), //pci
        0x2C => (*session_ptr).on_irq(0xC), //mouse
        0x2D => (*session_ptr).on_irq(0xD), //coprocessor
        0x2E => (*session_ptr).on_irq(0xE), //disk
        0x2F => (*session_ptr).on_irq(0xF), //disk
        0x80 => syscall_handle(regs),
        0xFF => {
            init(regs.ax);
            idle_loop();
        }
        0x0 => exception!("Divide by zero exception"),
        0x1 => exception!("Debug exception"),
        0x2 => exception!("Non-maskable interrupt"),
        0x3 => exception!("Breakpoint exception"),
        0x4 => exception!("Overflow exception"),
        0x5 => exception!("Bound range exceeded exception"),
        0x6 => exception!("Invalid opcode exception"),
        0x7 => exception!("Device not available exception"),
        0x8 => exception_error!("Double fault"),
        0xA => exception_error!("Invalid TSS exception"),
        0xB => exception_error!("Segment not present exception"),
        0xC => exception_error!("Stack-segment fault"),
        0xD => exception_error!("General protection fault"),
        0xE => exception_error!("Page fault"),
        0x10 => exception!("x87 floating-point exception"),
        0x11 => exception_error!("Alignment check exception"),
        0x12 => exception!("Machine check exception"),
        0x13 => exception!("SIMD floating-point exception"),
        0x14 => exception!("Virtualization exception"),
        0x1E => exception_error!("Security exception"),
        _ => exception!("Unknown Interrupt"),
    }
}
