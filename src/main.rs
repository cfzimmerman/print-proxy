use anyhow::{anyhow, bail};
use clap::Parser;
use printpdf::{
    image_crate::codecs::{jpeg::JpegDecoder, png::PngDecoder},
    Image, ImageTransform, Mm, PdfDocument, PdfDocumentReference,
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
    path::{Path, PathBuf},
};

/// CLI tool for generating printable PDFs of MTG proxy decks.
#[derive(clap::Parser)]
enum Args {
    /// MTG seems to have a de-facto txt format for transfering deck info.
    /// This takes info in the form of a Manabox txt export and converts it
    /// into a CSV with image URLs.
    ///
    /// Using this tool requires the env variable `MTG_API_KEY` for
    /// https://docs.magicthegathering.io to retrieve image urls.
    TxtToCsv {
        input_txt_path: PathBuf,
        output_csv_path: PathBuf,
    },
    CsvToPdf {
        input_csv_path: PathBuf,
        output_pdf_path: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args {
        Args::TxtToCsv {
            input_txt_path,
            output_csv_path,
        } => {
            ProxyCsv::csv_from_txt(&input_txt_path, &output_csv_path)?;
        }
        Args::CsvToPdf {
            input_csv_path,
            output_pdf_path,
        } => {
            let mut rows = csv::Reader::from_path(input_csv_path)?;
            let images = ProxyCsv::iter_csv_images(&mut rows)?;
            let doc = ProxyPdf::new();
            doc.gen_pdf(images)?;
            doc.save(output_pdf_path)?;
        }
    };
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

    fn make_image(mut image_bytes: impl Read, buf: &mut Vec<u8>) -> anyhow::Result<Image> {
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

#[derive(Serialize, Deserialize, Debug)]
struct DeckCsvRow {
    count: usize,
    card_name: String,
    image_url: String,
}

struct ProxyCsv {}

impl ProxyCsv {
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
            // can also be "png"
            .and_then(|uris| uris.get("normal"))
            .and_then(|val| val.as_str())
            .ok_or_else(|| anyhow!("Failed to extract image url from json output"))?;
        Ok(image_url.to_string())
    }

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

            let image_url = Self::get_image_url_for_card_name(&name).unwrap_or_else(|e| {
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

    pub fn iter_csv_images<'a, R: Read>(
        csv_reader: &'a mut csv::Reader<R>,
    ) -> anyhow::Result<impl Iterator<Item = Cursor<Vec<u8>>> + 'a> {
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
