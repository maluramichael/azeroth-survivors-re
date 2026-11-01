//! Stamps the commit this binary was built from, and on Windows gives the exe its icon and the
//! manifest the player build's dialogs need.

fn main() {
    benilla_buildstamp::emit();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_resources();
    }
}

/// The icon (`assets/windows/icon.ico`, drawn by `scripts/make-icon.py`) and an application
/// manifest: Common Controls 6, which rfd's dialogs with named buttons call into (`player.rs`),
/// and per-monitor DPI awareness, so Windows does not blur the dialogs on a scaled display.
fn windows_resources() {
    let icon = "../../assets/windows/icon.ico";
    println!("cargo:rerun-if-changed={icon}");
    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon).set_manifest(
        r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>"#,
    );
    if let Err(e) = res.compile() {
        panic!("windows resources: {e}");
    }
}
