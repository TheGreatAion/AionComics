//! PDF pages, from the reader built into Windows, so nothing extra ships with the app. A page comes
//! out as a picture, like any other comic page.

use std::path::Path;

use anyhow::Result;

/// How long a page's long side is drawn, in pixels. A PDF has no pixel size of its own.
const LONG_SIDE: f64 = 2400.0;

#[cfg(windows)]
mod windows_pdf {
    use super::*;
    use anyhow::{anyhow, bail};
    use windows::core::HSTRING;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::Win32::System::Com::CoIncrementMTAUsage;

    fn open(path: &Path) -> Result<PdfDocument> {
        static READY: std::sync::Once = std::sync::Once::new();
        // Lets any thread wait on Windows' reader, however that thread was started.
        READY.call_once(|| unsafe {
            let _ = CoIncrementMTAUsage();
        });
        let full = std::path::absolute(path)?.to_string_lossy().replace('/', "\\");
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(full))?.join()?;
        let doc = PdfDocument::LoadFromFileAsync(&file)?.join().map_err(|_| anyhow!("Windows couldn't read this PDF"))?;
        if doc.IsPasswordProtected()? {
            bail!("This PDF has a password");
        }
        Ok(doc)
    }

    pub fn page_count(path: &Path) -> Result<usize> {
        Ok(open(path)?.PageCount()? as usize)
    }

    /// One page as a PNG.
    pub fn page_png(path: &Path, index: usize) -> Result<Vec<u8>> {
        let doc = open(path)?;
        let page = doc.GetPage(index as u32)?;
        let size = page.Size()?;
        let scale = LONG_SIDE / f64::from(size.Width.max(size.Height)).max(1.0);
        let options = PdfPageRenderOptions::new()?;
        options.SetDestinationWidth((f64::from(size.Width) * scale) as u32)?;
        options.SetDestinationHeight((f64::from(size.Height) * scale) as u32)?;
        let stream = InMemoryRandomAccessStream::new()?;
        page.RenderWithOptionsToStreamAsync(&stream, &options)?.join()?;
        page.Close()?;
        let len = stream.Size()? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(len)?.join()?;
        let mut bytes = vec![0u8; len as usize];
        reader.ReadBytes(&mut bytes)?;
        Ok(bytes)
    }
}

#[cfg(windows)]
pub use windows_pdf::{page_count, page_png};

#[cfg(not(windows))]
pub fn page_count(_: &Path) -> Result<usize> {
    anyhow::bail!("PDFs can only be read on Windows for now")
}

#[cfg(not(windows))]
pub fn page_png(_: &Path, _: usize) -> Result<Vec<u8>> {
    anyhow::bail!("PDFs can only be read on Windows for now")
}
