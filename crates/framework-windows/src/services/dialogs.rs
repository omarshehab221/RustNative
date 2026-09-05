//! Native Win32 file dialogs (open, save, and folder pickers), realized with
//! the modern `IFileOpenDialog`/`IFileSaveDialog` COM interfaces — the
//! interface Microsoft's own documentation recommends over the classic
//! `GetOpenFileNameW`/`GetSaveFileNameW`/`SHBrowseForFolderW` functions this
//! module previously used, which still work but predate the current Windows
//! shell UI (no Quick Access sidebar, no modern styling, and — for
//! `SHBrowseForFolderW` specifically — a notably clunkier folder-picker UI
//! than `IFileOpenDialog` with `FOS_PICKFOLDERS` gives) (standards audit
//! P1.15).
//!
//! This is the one file in this crate that depends on the `windows` crate
//! rather than `windows-sys`: `windows-sys` deliberately ships flat FFI
//! bindings only, with no COM interface/vtable definitions at all (confirmed
//! by their total absence from its own source — there is no
//! `IFileOpenDialog` anywhere in it), so a `windows-sys`-only implementation
//! would mean hand-deriving this interface's GUID, vtable layout, and every
//! method's calling convention from documentation with no compiler or
//! bindings-crate help catching a transcription mistake — exactly the kind
//! of unverifiable-by-construction `unsafe` this crate otherwise avoids.
//! `windows` provides the real, maintained bindings for this interface
//! family; see `Cargo.toml` for why that dependency is scoped to just this
//! module rather than added workspace-wide.
//!
//! **Known remaining gap (`Audit.md`'s Phase 3 roadmap, item 15 — distinct
//! from the "P1.15" severity-finding numbering used in `BUILD_STATUS.md`,
//! and not yet addressed):** every dialog here is shown with no owner window (`IFileDialog::Show(None)`),
//! matching this module's previous behavior. `FileDialogRequest` (in
//! `framework-core`, shared across every platform backend) currently has no
//! field to carry a window identity through, so giving these dialogs a real
//! parent/owner would mean extending that cross-platform type first — a
//! larger, deliberate API change belonging to its own pass, not something to
//! fold silently into a same-behavior backend swap.

#[cfg(windows)]
use super::run_sta;

#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsFileDialogs;

#[cfg(windows)]
#[async_trait::async_trait]
impl framework_core::FileDialogService for WindowsFileDialogs {
    async fn show(
        &self,
        request: framework_core::FileDialogRequest,
    ) -> Result<Option<String>, framework_core::ServiceError> {
        // A file picker owns an STA for its complete modal lifetime. It is
        // intentionally not run on Tokio's reusable blocking pool.
        run_sta(move || show_file_dialog(&request)).await
    }
}

#[cfg(windows)]
fn show_file_dialog(
    request: &framework_core::FileDialogRequest,
) -> Result<Option<String>, framework_core::ServiceError> {
    use framework_core::FileDialogKind;
    // `run_sta` has already established a dedicated STA and pairs its COM
    // initialization (via `windows-sys`'s `CoInitializeEx`) with teardown
    // after this synchronous dialog returns; COM apartment state is
    // process/thread-level OS state, not tied to which bindings crate
    // observes it, so `windows`'s `CoCreateInstance` below runs within that
    // same already-initialized apartment without needing its own init call.
    match request.kind {
        FileDialogKind::OpenFile => show_open(request),
        FileDialogKind::SaveFile => show_save(request),
        FileDialogKind::PickFolder => show_pick_folder(request),
    }
}

/// The documented `HRESULT` `IFileDialog::Show` returns when the person
/// dismisses the dialog without making a selection (`HRESULT_FROM_WIN32(
/// ERROR_CANCELLED)`) — every caller below treats this one specific error as
/// "the person cancelled" (`Ok(None)`), and every other `Err` as a real
/// failure worth reporting.
#[cfg(windows)]
const ERROR_CANCELLED_HRESULT: i32 = {
    // `HRESULT_FROM_WIN32(ERROR_CANCELLED)` (`0x8007_04C7`) is a documented
    // Win32 constant whose top bit is deliberately set (every
    // `HRESULT_FROM_WIN32`-derived failure code has it set, by
    // definition) — the bit-pattern reinterpretation this cast performs
    // is exactly the value Microsoft's own documentation specifies, not
    // an accidental wraparound.
    #[allow(clippy::cast_possible_wrap)]
    let value = 0x8007_04C7u32 as i32;
    value
};

#[cfg(windows)]
fn is_user_cancelled(error: &windows::core::Error) -> bool {
    error.code().0 == ERROR_CANCELLED_HRESULT
}

#[cfg(windows)]
fn show_open(
    request: &framework_core::FileDialogRequest,
) -> Result<Option<String>, framework_core::ServiceError> {
    use windows::Win32::System::Com::CLSCTX_INPROC_SERVER;
    use windows::Win32::UI::Shell::{
        FOS_FILEMUSTEXIST, FOS_PATHMUSTEXIST, FileOpenDialog, IFileOpenDialog,
    };
    use windows::core::Interface;

    // SAFETY: `FileOpenDialog` is a documented, always-registered
    // in-process shell COM class; requesting the `IFileOpenDialog`
    // interface from it is exactly its documented purpose.
    let dialog: IFileOpenDialog = unsafe {
        windows::Win32::System::Com::CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
    }
    .map_err(|error| to_service_error("CoCreateInstance(FileOpenDialog)", &error))?;

    let file_dialog: windows::Win32::UI::Shell::IFileDialog = dialog
        .cast()
        .map_err(|error| to_service_error("IFileOpenDialog::cast to IFileDialog", &error))?;
    configure_common_options(&file_dialog, request, FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST)?;

    // SAFETY: `dialog` is a live `IFileOpenDialog` just created and
    // configured above; a null owner (`None`) is a documented-valid
    // argument (see this module's doc comment on the P3.15 gap).
    match unsafe { dialog.Show(None) } {
        Ok(()) => {}
        Err(error) if is_user_cancelled(&error) => return Ok(None),
        Err(error) => return Err(to_service_error("IFileOpenDialog::Show", &error)),
    }

    // SAFETY: `Show` just returned successfully, which is `GetResult`'s
    // documented precondition for having a result to return.
    let item = unsafe { dialog.GetResult() }
        .map_err(|error| to_service_error("IFileOpenDialog::GetResult", &error))?;
    resolve_file_system_path(&item)
}

#[cfg(windows)]
fn show_save(
    request: &framework_core::FileDialogRequest,
) -> Result<Option<String>, framework_core::ServiceError> {
    use windows::Win32::System::Com::CLSCTX_INPROC_SERVER;
    use windows::Win32::UI::Shell::{FOS_OVERWRITEPROMPT, FileSaveDialog, IFileSaveDialog};
    use windows::core::Interface;

    // SAFETY: `FileSaveDialog` is a documented, always-registered
    // in-process shell COM class; requesting the `IFileSaveDialog`
    // interface from it is exactly its documented purpose.
    let dialog: IFileSaveDialog = unsafe {
        windows::Win32::System::Com::CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)
    }
    .map_err(|error| to_service_error("CoCreateInstance(FileSaveDialog)", &error))?;

    let file_dialog: windows::Win32::UI::Shell::IFileDialog = dialog
        .cast()
        .map_err(|error| to_service_error("IFileSaveDialog::cast to IFileDialog", &error))?;
    configure_common_options(&file_dialog, request, FOS_OVERWRITEPROMPT)?;

    // SAFETY: `dialog` is a live `IFileSaveDialog` just created and
    // configured above; a null owner (`None`) is a documented-valid
    // argument (see this module's doc comment on the P3.15 gap).
    match unsafe { dialog.Show(None) } {
        Ok(()) => {}
        Err(error) if is_user_cancelled(&error) => return Ok(None),
        Err(error) => return Err(to_service_error("IFileSaveDialog::Show", &error)),
    }

    // SAFETY: `Show` just returned successfully, which is `GetResult`'s
    // documented precondition for having a result to return.
    let item = unsafe { dialog.GetResult() }
        .map_err(|error| to_service_error("IFileSaveDialog::GetResult", &error))?;
    resolve_file_system_path(&item)
}

#[cfg(windows)]
fn show_pick_folder(
    request: &framework_core::FileDialogRequest,
) -> Result<Option<String>, framework_core::ServiceError> {
    use windows::Win32::System::Com::CLSCTX_INPROC_SERVER;
    use windows::Win32::UI::Shell::{
        FOS_PICKFOLDERS, FileOpenDialog, IFileDialog, IFileOpenDialog,
    };
    use windows::core::Interface;

    // SAFETY: `FileOpenDialog` is a documented, always-registered
    // in-process shell COM class. `IFileOpenDialog` with the
    // `FOS_PICKFOLDERS` option is Microsoft's own documented modern
    // replacement for `SHBrowseForFolderW`, which this crate previously
    // used for folder picking.
    let dialog: IFileOpenDialog = unsafe {
        windows::Win32::System::Com::CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
    }
    .map_err(|error| to_service_error("CoCreateInstance(FileOpenDialog)", &error))?;

    let file_dialog: IFileDialog = dialog
        .cast()
        .map_err(|error| to_service_error("IFileOpenDialog::cast to IFileDialog", &error))?;
    configure_common_options(&file_dialog, request, FOS_PICKFOLDERS)?;

    // SAFETY: `dialog` is a live `IFileOpenDialog` just created and
    // configured above; a null owner (`None`) is a documented-valid
    // argument (see this module's doc comment on the P3.15 gap).
    match unsafe { dialog.Show(None) } {
        Ok(()) => {}
        Err(error) if is_user_cancelled(&error) => return Ok(None),
        Err(error) => return Err(to_service_error("IFileOpenDialog::Show", &error)),
    }

    // SAFETY: `Show` just returned successfully, which is `GetResult`'s
    // documented precondition for having a result to return.
    let item = unsafe { dialog.GetResult() }
        .map_err(|error| to_service_error("IFileOpenDialog::GetResult", &error))?;
    resolve_file_system_path(&item)
}

/// Applies the title, file-type filters, and dialog-specific option flags
/// every dialog kind shares, via the common `IFileDialog` base interface
/// every specific dialog interface inherits.
#[cfg(windows)]
fn configure_common_options(
    dialog: &windows::Win32::UI::Shell::IFileDialog,
    request: &framework_core::FileDialogRequest,
    extra_options: windows::Win32::UI::Shell::FILEOPENDIALOGOPTIONS,
) -> Result<(), framework_core::ServiceError> {
    use windows::Win32::UI::Shell::FOS_FORCEFILESYSTEM;
    use windows::core::HSTRING;

    if let Some(title) = &request.title {
        // SAFETY: `dialog` is a live `IFileDialog`; `HSTRING::from(title)`
        // owns its own null-terminated buffer for the duration of this
        // call (the temporary's lifetime is extended by Rust to cover the
        // whole statement).
        unsafe { dialog.SetTitle(&HSTRING::from(title.as_str())) }
            .map_err(|error| to_service_error("IFileDialog::SetTitle", &error))?;
    }

    // SAFETY: `dialog` is a live `IFileDialog`. Every dialog kind requires
    // `FOS_FORCEFILESYSTEM` (Microsoft's documented guidance: without it,
    // `GetResult` can return non-file-system shell items — e.g. Recycle
    // Bin — that have no path this framework's `String`-typed result could
    // represent) in addition to its own specific option flags.
    unsafe { dialog.SetOptions(FOS_FORCEFILESYSTEM | extra_options) }
        .map_err(|error| to_service_error("IFileDialog::SetOptions", &error))?;

    if !request.filters.is_empty() {
        let specs: Vec<(HSTRING, HSTRING)> = request
            .filters
            .iter()
            .map(|(label, patterns)| {
                let pattern = if patterns.is_empty() {
                    "*.*".to_owned()
                } else {
                    patterns
                        .iter()
                        .map(|extension| format!("*.{extension}"))
                        .collect::<Vec<_>>()
                        .join(";")
                };
                (HSTRING::from(label.as_str()), HSTRING::from(pattern))
            })
            .collect();
        let raw_specs: Vec<windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC> = specs
            .iter()
            .map(|(name, spec)| windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC {
                pszName: windows::core::PCWSTR::from_raw(name.as_ptr()),
                pszSpec: windows::core::PCWSTR::from_raw(spec.as_ptr()),
            })
            .collect();
        // SAFETY: `dialog` is a live `IFileDialog`; `raw_specs` borrows
        // from `specs`, which (along with `raw_specs` itself) outlives
        // this call, and `SetFileTypes` documents that it copies the
        // filter data it needs rather than retaining these pointers past
        // the call's return.
        unsafe { dialog.SetFileTypes(&raw_specs) }
            .map_err(|error| to_service_error("IFileDialog::SetFileTypes", &error))?;
    }

    Ok(())
}

/// Resolves a selected `IShellItem` to the plain file-system path string
/// this framework's cross-platform `FileDialogService` trait returns.
#[cfg(windows)]
fn resolve_file_system_path(
    item: &windows::Win32::UI::Shell::IShellItem,
) -> Result<Option<String>, framework_core::ServiceError> {
    use windows::Win32::UI::Shell::SIGDN_FILESYSPATH;

    // SAFETY: `item` is a live `IShellItem` (the caller obtained it from a
    // successful `GetResult`); `SIGDN_FILESYSPATH` is a documented, valid
    // display-name kind requesting a plain file-system path.
    let path = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }
        .map_err(|error| to_service_error("IShellItem::GetDisplayName", &error))?;
    // SAFETY: `path` is the `PWSTR` `GetDisplayName` just returned above,
    // a NUL-terminated wide string this call takes ownership of; `.to_string()`
    // reads it (never past the terminator) without retaining the pointer,
    // and it is deallocated via `CoTaskMemFree` immediately after,
    // pairing `GetDisplayName`'s documented "caller must free with
    // `CoTaskMemFree`" contract exactly once.
    let text = unsafe { path.to_string() }
        .map_err(|error| framework_core::ServiceError::new(format!("PWSTR::to_string: {error}")))?;
    // SAFETY: `path.0` is the same `CoTaskMemAlloc`-allocated pointer
    // `GetDisplayName` returned, freed here exactly once, after its last
    // use (`to_string` above).
    unsafe {
        windows::Win32::System::Com::CoTaskMemFree(Some(path.0.cast()));
    }
    Ok(Some(text))
}

#[cfg(windows)]
fn to_service_error(operation: &str, error: &windows::core::Error) -> framework_core::ServiceError {
    framework_core::ServiceError::new(format!("{operation} failed: {error}"))
}
