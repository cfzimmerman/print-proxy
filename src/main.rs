use printpdf::{image_crate::codecs::jpeg::JpegDecoder, Image, ImageTransform, Mm, PdfDocument};
use std::{
    fs::File,
    io::{BufWriter, Cursor},
};

// const IMAGE_URL: &str = "https://cards.scryfall.io/normal/front/3/c/3c558349-87bc-4e0f-96c2-b075f7da97d5.jpg?1712356813";
const PAGE_HEIGHT: f32 = 279.;
const PAGE_WIDTH: f32 = 210.;

const CARD_HEIGHT_MM: f32 = 88.9;
const CARD_WIDTH_MM: f32 = 63.5;
const DPI: f32 = 300.;

fn gen_pdf() -> anyhow::Result<()> {
    let (doc, page1, layer1) =
        PdfDocument::new("test_doc", Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "layer1");

    let current_layer = doc.get_page(page1).get_layer(layer1);

    let image_bytes = include_bytes!("../wakeen.jpg");
    let mut reader = Cursor::new(&image_bytes);

    let decoder = JpegDecoder::new(&mut reader)?;
    let image = Image::try_from(decoder)?;

    let height_mm = Mm::from(image.image.height.into_pt(DPI)).0;
    let width_mm = Mm::from(image.image.width.into_pt(DPI)).0;

    let height_scale = CARD_HEIGHT_MM / height_mm;
    let width_scale = CARD_WIDTH_MM / width_mm;

    // layer,
    image.add_to_layer(
        current_layer.clone(),
        ImageTransform {
            translate_x: Some(Mm(10.0)),
            translate_y: Some(Mm(10.0)),
            scale_x: Some(width_scale),
            scale_y: Some(height_scale),
            dpi: Some(DPI),
            ..Default::default()
        },
    );

    doc.save(&mut BufWriter::new(File::create("test_output.pdf")?))?;

    Ok(())
}

fn main() -> anyhow::Result<()> {
    gen_pdf()
}
