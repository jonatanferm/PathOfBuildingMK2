//! Web [`Net`] over `fetch` (ui doc §5.2, §13 risk 6: CORS limits import, trade and OAuth;
//! wasm32 only). Owner: U7.

use pob_sg::{HttpRequest, HttpResponse, Net};

#[derive(Default)]
pub struct WebNet;

impl Net for WebNet {
    fn download_page(&mut self, req: HttpRequest) -> u64 {
        let _ = req;
        todo!("U7: fetch-based DownloadPage")
    }
    fn poll(&mut self) -> Vec<(u64, HttpResponse)> {
        Vec::new()
    }
    fn busy(&self) -> bool {
        false
    }
}
