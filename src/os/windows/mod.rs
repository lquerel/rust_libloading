/// Windows implementation of dynamic library loading.
///
/// This module should be expanded with more Windows-specific functionality in the future.

extern crate winapi;
extern crate kernel32;

use std::ffi::{CStr, OsStr};
use std::marker;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::os::windows::ffi::OsStrExt;


struct Module(pub winapi::HMODULE);

impl Drop for Module {
    fn drop(&mut self) {
        with_get_last_error(|| {
            if unsafe { kernel32::FreeLibrary(self.0) == 0 } {
                None
            } else {
                Some(())
            }
        }).unwrap()
    }
}


/// A platform-specific equivalent of the cross-platform `Library`.
pub struct Library(Module);

impl Library {
    /// Find and load a shared library (module).
    ///
    /// Locations where library is searched for is platform specific and can’t be adjusted
    /// portably.
    ///
    /// Corresponds to `LoadLibraryW(filename)`.
    #[inline]
    pub fn new<P: AsRef<OsStr>>(filename: P) -> ::Result<Library> {
        let mut wide_filename: Vec<u16> = filename.as_ref().encode_wide().collect();
        wide_filename.push(0);
        wide_filename.shrink_to_fit();
        let _guard = ErrorModeGuard::new();

        let ret = with_get_last_error(|| {
            // Make sure no winapi calls as a result of drop happen inside this closure, because
            // otherwise that might change the return value of the GetLastError.
            let handle = unsafe { kernel32::LoadLibraryW(wide_filename.as_ptr()) };
            if handle.is_null()  {
                None
            } else {
                Some(Library(Module(handle)))
            }
        }).map_err(|e| e.unwrap_or_else(||
            panic!("LoadLibraryW failed but GetLastError did not report the error")
        ));

        drop(wide_filename); // Drop wide_filename here to ensure it doesn’t get moved and dropped
                             // inside the closure by mistake.
        ret
    }

    /// Get a symbol by name.
    ///
    /// Mangling or symbol rustification is not done: trying to `get` something like `x::y`
    /// will not work.
    ///
    /// # Unsafety
    ///
    /// The pointer to a symbol of arbitrary type or kind is returned. Requesting for function
    /// pointer while the symbol is not one and vice versa is not memory safe.
    ///
    /// The return value does not ensure the symbol does not outlive the library.
    pub unsafe fn get<T>(&self, symbol: &CStr) -> ::Result<Symbol<T>> {
        with_get_last_error(|| {
            let symbol = kernel32::GetProcAddress((self.0).0, symbol.as_ptr());
            if symbol.is_null() {
                None
            } else {
                Some(Symbol {
                    pointer: symbol,
                    pd: marker::PhantomData
                })
            }
        }).map_err(|e| e.unwrap_or_else(||
            panic!("GetProcAddress failed but GetLastError did not report the error")
        ))
    }
}


pub struct Symbol<T> {
    pointer: winapi::FARPROC,
    pd: marker::PhantomData<T>
}

impl<T> ::std::ops::Deref for Symbol<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe {
            &*(&self.pointer as *const _ as *const T)
        }
    }
}


static USE_THREADERRORMODE: AtomicBool = AtomicBool::new(true);
struct ErrorModeGuard(winapi::DWORD);

impl ErrorModeGuard {
    fn new() -> ErrorModeGuard {
        let mut ret = ErrorModeGuard(0);

        if USE_THREADERRORMODE.load(Ordering::Acquire) {
            if unsafe { kernel32::SetThreadErrorMode(1, &mut ret.0) == 0
                        && kernel32::GetLastError() == winapi::ERROR_CALL_NOT_IMPLEMENTED } {
                USE_THREADERRORMODE.store(false, Ordering::Release);
            } else {
                return ret;
            }
        }
        ret.0 = unsafe { kernel32::SetErrorMode(1) };
        ret
    }
}

impl Drop for ErrorModeGuard {
    fn drop(&mut self) {
        unsafe {
            if USE_THREADERRORMODE.load(Ordering::Relaxed) {
                kernel32::SetThreadErrorMode(self.0, ptr::null_mut());
            } else {
                kernel32::SetErrorMode(self.0);
            }
        }
    }
}


fn with_get_last_error<T, F>(closure: F) -> Result<T, Option<String>>
where F: FnOnce() -> Option<T> {
    closure().map(Ok).unwrap_or_else(|| {
        let error = unsafe { kernel32::GetLastError() };
        Err(if error == 0 {
            None
        } else {
            // TODO: Possibly use FormatMessage (lots of work here)
            Some(format!("Error {}", error))
        })
    })
}


#[test]
fn works_new_kernel32() {
    let that = Library::new("kernel32.dll").unwrap();
    unsafe {
        that.get::<*mut usize>(&::std::ffi::CString::new("GetLastError").unwrap()).unwrap();
    }
}

#[test]
fn fails_new_kernel23() {
    Library::new("kernel23").err().unwrap();
}