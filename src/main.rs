use anyhow::{anyhow, bail};
use printpdf::{
    image_crate::codecs::{jpeg::JpegDecoder, png::PngDecoder},
    Image, ImageTransform, Mm, PdfDocument, PdfDocumentReference,
};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Cursor, Read},
    path::Path,
};

// const IMAGE_URL: &str = "https://cards.scryfall.io/normal/front/3/c/3c558349-87bc-4e0f-96c2-b075f7da97d5.jpg?1712356813";

fn main() -> anyhow::Result<()> {
    let doc = ProxyPdf::new();
    let images = (0..24).map_while(|_| {
        let reader = BufReader::new(
            File::open("./wakeen.jpg")
                .inspect_err(|e| eprintln!("{e:?}"))
                .ok()?,
        );
        Some(reader)
    });
    doc.gen_pdf(images)?;
    doc.save("test_output.pdf")?;
    Ok(())
}

struct ProxyPdf {
    pdf: PdfDocumentReference,
}

impl ProxyPdf {
    const PAGE_HEIGHT_MM: f32 = 279.;
    const PAGE_WIDTH_MM: f32 = 210.;

    const CARD_HEIGHT_MM: f32 = 88.9;
    const CARD_WIDTH_MM: f32 = 63.5;
    const DPI: f32 = 300.;

    const HEIGHT_MARGIN_MM: f32 =
        (Self::PAGE_HEIGHT_MM - (3. * Self::CARD_HEIGHT_MM) - (2. * Self::MARGIN_BETWEEN_CARDS_MM))
            / 2.;
    const WIDTH_MARGIN_MM: f32 =
        (Self::PAGE_WIDTH_MM - (3. * Self::CARD_WIDTH_MM) - (2. * Self::MARGIN_BETWEEN_CARDS_MM))
            / 2.;
    const MARGIN_BETWEEN_CARDS_MM: f32 = 1.;

    pub fn new() -> Self {
        Self {
            pdf: PdfDocument::empty("MTG deck proxy"),
        }
    }

    pub fn save(self, output_file: impl AsRef<Path>) -> anyhow::Result<()> {
        Ok(self
            .pdf
            .save(&mut BufWriter::new(File::create(output_file)?))?)
    }

    pub fn gen_pdf<'a, R: Read>(&self, images: impl Iterator<Item = R> + 'a) -> anyhow::Result<()> {
        let mut pages_this_doc = 0;
        let mut cards_this_page = 8;
        let mut current_layer = None;

        let mut buf = Vec::new();
        for image_bytes in images {
            cards_this_page = (cards_this_page + 1) % 9;
            let row = cards_this_page / 3;
            let col = cards_this_page % 3;

            if row == 0 && col == 0 {
                pages_this_doc += 1;
                let (page_idx, layer_idx) = self.pdf.add_page(
                    Mm(Self::PAGE_WIDTH_MM),
                    Mm(Self::PAGE_HEIGHT_MM),
                    format!("page{pages_this_doc}"),
                );
                current_layer = Some(self.pdf.get_page(page_idx).get_layer(layer_idx));
            }

            let image = Self::make_image(image_bytes, &mut buf)?;
            let height_mm = Mm::from(image.image.height.into_pt(Self::DPI)).0;
            let width_mm = Mm::from(image.image.width.into_pt(Self::DPI)).0;

            let height_scale = Self::CARD_HEIGHT_MM / height_mm;
            let width_scale = Self::CARD_WIDTH_MM / width_mm;
            let (col32, row32) = (col as f32, row as f32);

            let col_cardspace = f32::ceil(col32 / 1.) * Self::MARGIN_BETWEEN_CARDS_MM;
            let row_cardspace = f32::ceil(row32 / 1.) * Self::MARGIN_BETWEEN_CARDS_MM;

            image.add_to_layer(
                current_layer
                    .as_ref()
                    .expect("Prev steps should guarantee layer is present")
                    .clone(),
                ImageTransform {
                    translate_x: Some(Mm(col32 * Self::CARD_WIDTH_MM
                        + col_cardspace
                        + Self::WIDTH_MARGIN_MM)),
                    translate_y: Some(Mm(row32 * Self::CARD_HEIGHT_MM
                        + row_cardspace
                        + Self::HEIGHT_MARGIN_MM)),
                    scale_x: Some(width_scale),
                    scale_y: Some(height_scale),
                    dpi: Some(Self::DPI),
                    rotate: None,
                },
            );
        }

        Ok(())
    }

    pub fn make_image(mut image_bytes: impl Read, buf: &mut Vec<u8>) -> anyhow::Result<Image> {
        use image::{ImageFormat, ImageReader};

        buf.clear();
        image_bytes.read_to_end(buf)?;
        let img_fmt = ImageReader::new(Cursor::new(buf.as_slice()))
            .with_guessed_format()?
            .format()
            .ok_or_else(|| anyhow!("Unable to detect the png/jpg format of this image"));

        match img_fmt? {
            ImageFormat::Jpeg => Ok(Image::try_from(JpegDecoder::new(buf.as_slice())?)?),
            ImageFormat::Png => Ok(Image::try_from(PngDecoder::new(buf.as_slice())?)?),
            format => {
                bail!("unsupported image format: {format:?}");
            }
        }
    }
}
