//! Web [`Platform`] (ui doc §4.6, §5.2 "Web host"; wasm32 only). Owner: U7.

use pob_sg::Platform;

/// Browser services (performance.now clock, DOM clipboard, window.open, document.title).
#[derive(Default)]
pub struct WebPlatform;

impl Platform for WebPlatform {
    fn time_ms(&self) -> i64 {
        todo!("U7: web GetTime")
    }
    fn copy(&mut self, text: &str) {
        let _ = text;
        todo!("U7: navigator.clipboard.writeText")
    }
    fn paste(&mut self) -> Option<String> {
        todo!("U7: cached DOM paste text")
    }
    fn open_url(&mut self, url: &str) {
        let _ = url;
        todo!("U7: window.open")
    }
    fn set_window_title(&mut self, title: &str) {
        let _ = title;
        todo!("U7: document.title")
    }
    fn take_screenshot(&mut self) {
        todo!("U7: web TakeScreenshot")
    }
    fn restart(&mut self) {
        todo!("U7: web Restart")
    }
    fn exit(&mut self, msg: Option<&str>) {
        let _ = msg;
        todo!("U7: web Exit")
    }
    fn script_path(&self) -> String {
        todo!("U7: web GetScriptPath")
    }
    fn runtime_path(&self) -> String {
        todo!("U7: web GetRuntimePath")
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        todo!("U7: web GetUserPath")
    }
    fn con_print(&mut self, s: &str) {
        let _ = s;
        todo!("U7: console.log")
    }
}
