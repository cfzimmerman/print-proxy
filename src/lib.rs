use anyhow::anyhow;
use printpdf::{
    image_crate::codecs::jpeg::JpegDecoder, Image, ImageTransform, Mm, PdfDocument,
    PdfDocumentReference,
};
use reqwest::{
    blocking,
    header::{ACCEPT, USER_AGENT},
    Url,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Cursor, Read},
    path::Path,
};

/// These are the fields expected in a CSV row
/// used for proxy generation.
#[derive(Serialize, Deserialize, Debug)]
struct DeckCsvRow {
    count: usize,
    card_name: String,
    image_url: String,
}

/// Struct for creating PDFs with American-sized MTG cards
/// arranged 3x3 on normal printer paper.
pub struct ProxyPdf {
    pdf: PdfDocumentReference,
}

impl ProxyPdf {
    /// Absolute doc dimensions (MM = millimeters everywhere)
    const PAGE_HEIGHT_MM: f32 = 279.;
    const PAGE_WIDTH_MM: f32 = 210.;

    /// Dimensions of an MTG card. Undersized by 2 mm so they fit better in a card sleeve
    const CARD_HEIGHT_MM: f32 = 86.9;
    const CARD_WIDTH_MM: f32 = 61.5;

    /// Pixel density in images
    const DPI: f32 = 300.;

    /// How much space is on the document's borders
    const HEIGHT_MARGIN_MM: f32 =
        (Self::PAGE_HEIGHT_MM - (3. * Self::CARD_HEIGHT_MM) - (2. * Self::MARGIN_BETWEEN_CARDS_MM))
            / 2.;
    const WIDTH_MARGIN_MM: f32 =
        (Self::PAGE_WIDTH_MM - (3. * Self::CARD_WIDTH_MM) - (2. * Self::MARGIN_BETWEEN_CARDS_MM))
            / 2.;

    /// How much space is between cards
    const MARGIN_BETWEEN_CARDS_MM: f32 = 1.;

    /// Creates a new pdf. Remember to call `.save` when finished.
    pub fn new() -> Self {
        Self::default()
    }

    /// Saves the PDF to the given file path. Use a `.pdf` file ending.
    pub fn save(self, output_file: impl AsRef<Path>) -> anyhow::Result<()> {
        Ok(self
            .pdf
            .save(&mut BufWriter::new(File::create(output_file)?))?)
    }

    /// Builds an MTG proxy from the iterator. This assumes every iterator
    /// item is the bytes of an image.
    pub fn gen_pdf<'a, R: Read>(&self, images: impl Iterator<Item = R> + 'a) -> anyhow::Result<()> {
        let mut pages_this_doc = 0;
        let mut cards_this_page = 8;
        let mut current_layer = None;

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

            let image = Image::try_from(JpegDecoder::new(image_bytes)?)?;
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
}

impl Default for ProxyPdf {
    fn default() -> Self {
        Self {
            pdf: PdfDocument::empty("MTG deck proxy"),
        }
    }
}

pub struct ProxyCsv {}

impl ProxyCsv {
    /// Queries scryfall for the image url associated with this MTG card
    /// (if one can be found).
    fn get_image_url_for_card_name(name: &str) -> anyhow::Result<String> {
        let url = Url::parse(&format!(
            "https://api.scryfall.com/cards/named?exact={name}"
        ))?;

        let client = blocking::Client::new();
        let card_info: Value = client
            .get(url)
            .header(USER_AGENT, "MyCliProxyFormatter/1.0")
            .header(ACCEPT, "*/*")
            .send()?
            .json()?;

        let image_url = card_info
            .get("image_uris")
            .and_then(|uris| uris.get("normal"))
            .and_then(|val| val.as_str())
            .ok_or_else(|| anyhow!("Failed to extract image url from json output"))?;
        Ok(image_url.to_string())
    }

    /// Parses a manabox-style text file into a CSV usable by pdf gen.
    pub fn csv_from_txt(input_txt: &Path, output_csv: &Path) -> anyhow::Result<()> {
        let mut out = csv::Writer::from_path(output_csv)?;
        for line in BufReader::new(File::open(input_txt)?).lines() {
            let line = line?;
            let mut words = line.trim().splitn(2, ' ');
            let count = words.next();
            let name = words.next();

            let Some((count, name)) = count
                .and_then(|word| word.parse::<usize>().ok())
                .and_then(|ct| name.map(|n| (ct, n)))
            else {
                println!("skipping: {line}");
                continue;
            };

            let image_url = Self::get_image_url_for_card_name(name).unwrap_or_else(|e| {
                eprintln!("Image fetch failed: {e:?}");
                String::new()
            });

            println!("adding x{count} {name} at {image_url}");
            out.serialize(DeckCsvRow {
                count,
                card_name: name.to_string(),
                image_url,
            })?;
        }
        Ok(())
    }

    /// Iterates the rows of the CSV, yielding one image buffer per card
    /// required in the deck.
    /// This isn't very memory efficient, but we don't really need that here.
    pub fn iter_csv_images<R: Read>(
        csv_reader: &mut csv::Reader<R>,
    ) -> anyhow::Result<impl Iterator<Item = Cursor<Vec<u8>>> + '_> {
        let results = csv_reader
            .deserialize()
            .map_while(|row| {
                let row: DeckCsvRow = row
                    .inspect_err(|e| eprintln!("Malformed csv row: {e:?}"))
                    .ok()?;
                println!("{row:?}");
                let fetched_image = blocking::get(&row.image_url)
                    .inspect_err(|e| eprintln!("image fetch failed: {e:?}"))
                    .ok()?;
                if !fetched_image.status().is_success() {
                    eprintln!("fetch failed: {:?}", fetched_image.status());
                    return None;
                }
                let bytes = fetched_image
                    .bytes()
                    .inspect_err(|e| eprintln!("failed to fetch response bytes: {e:?}"))
                    .ok()?
                    .to_vec();

                Some((0..row.count).map(move |_| Cursor::new(bytes.clone())))
            })
            .flatten();
        Ok(results)
    }
}
