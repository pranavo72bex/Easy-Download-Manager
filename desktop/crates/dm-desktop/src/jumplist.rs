//! The taskbar button's jump list on Windows: a right-click on dm's icon
//! offers "Add URL" (which opens that dialog in the running dm) and "Open
//! downloads folder".
//!
//! Made on a thread of its own and every failure ignored: a jump list is a
//! convenience, and a shell that will not make one must not cost dm its
//! start. Elsewhere there is no such thing, and [`publish`] does nothing.

/// Puts the two entries on dm's jump list, named in the language in force.
/// `folder` is where downloads go.
#[cfg(windows)]
pub fn publish(new_download: &str, open_folder: &str, folder: &std::path::Path) {
    let (new_download, open_folder, folder) = (
        new_download.to_string(),
        open_folder.to_string(),
        folder.to_path_buf(),
    );
    let _ = std::thread::Builder::new()
        .name("dm-jump-list".into())
        .spawn(move || {
            let _ = build(&new_download, &open_folder, &folder);
        });
}

#[cfg(not(windows))]
pub fn publish(_new_download: &str, _open_folder: &str, _folder: &std::path::Path) {}

#[cfg(windows)]
fn build(
    new_download: &str,
    open_folder: &str,
    folder: &std::path::Path,
) -> windows::core::Result<()> {
    use windows::core::PWSTR;
    use windows::core::{Interface, HSTRING};
    use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
    use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{
        DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
    };

    /// One entry: a shell link to `target` with `arguments`, titled `title`.
    fn entry(
        title: &str,
        target: &str,
        arguments: &str,
        icon: &str,
        icon_at: i32,
    ) -> windows::core::Result<IShellLinkW> {
        // SAFETY: COM calls on interfaces made here, on the thread that
        // initialised COM; the strings outlive each call.
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(&HSTRING::from(target))?;
            if !arguments.is_empty() {
                link.SetArguments(&HSTRING::from(arguments))?;
            }
            link.SetIconLocation(&HSTRING::from(icon), icon_at)?;
            let store: IPropertyStore = link.cast()?;
            // The title as a string the property store owns (VT_LPWSTR, in
            // memory COM frees), which `PropVariantClear` gives back.
            let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
            let text = CoTaskMemAlloc(wide.len() * 2).cast::<u16>();
            if text.is_null() {
                return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), text, wide.len());
            let mut name = PROPVARIANT::default();
            let inner = &mut *name.Anonymous.Anonymous;
            inner.vt = VT_LPWSTR;
            inner.Anonymous.pwszVal = PWSTR(text);
            let set = store.SetValue(&PKEY_Title, &name);
            let _ = PropVariantClear(&mut name);
            set?;
            store.Commit()?;
            Ok(link)
        }
    }

    let exe = std::env::current_exe()
        .map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
    let exe = exe.display().to_string();
    let folder = folder.display().to_string();

    // SAFETY: COM is initialised for this thread and undone at its end; every
    // interface is used on this thread only.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let made = (|| -> windows::core::Result<()> {
            let list: ICustomDestinationList =
                CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
            let mut slots = 0u32;
            let _removed: IObjectArray = list.BeginList(&mut slots)?;
            let tasks: IObjectCollection =
                CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
            tasks.AddObject(&entry(new_download, &exe, "--new-download", &exe, 0)?)?;
            // The folder's own shell link: Explorer opens it. The folder
            // icon is the system's (shell32, entry 3).
            tasks.AddObject(&entry(open_folder, &folder, "", "shell32.dll", 3)?)?;
            list.AddUserTasks(&tasks.cast::<IObjectArray>()?)?;
            list.CommitList()
        })();
        CoUninitialize();
        made
    }
}
