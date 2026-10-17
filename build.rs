// Embeds the icon and version info in updateeer.exe (Windows only; elsewhere this does nothing).
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Updateeer");
        res.set("FileDescription", "Updateeer");
        res.compile().expect("failed to embed the Windows resources");
    }
}
