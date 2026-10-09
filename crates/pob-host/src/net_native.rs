//! Native [`Net`] (ui doc §4.6, §5.2): `launch:DownloadPage` (Launch.lua:259-324) over the ureq
//! logic of `__mk2_http`, plus `oauth_listen` lifted from `crates/pob-oracle/src/jit.rs:29-132`.
//! Owner: U7. UI-U0 stub: nothing is ever in flight until U7 implements `download_page`.

use pob_sg::{HttpRequest, HttpResponse, Net};

#[derive(Default)]
pub struct NativeNet;

impl Net for NativeNet {
    fn download_page(&mut self, req: HttpRequest) -> u64 {
        let _ = req;
        todo!("U7: launch:DownloadPage (Launch.lua:259-324)")
    }
    fn poll(&mut self) -> Vec<(u64, HttpResponse)> {
        Vec::new()
    }
    fn busy(&self) -> bool {
        false
    }
}
