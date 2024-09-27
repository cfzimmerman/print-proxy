use clap::Parser;
use print_proxy::{ProxyCsv, ProxyPdf};
use std::path::PathBuf;

/// CLI tool for generating printable PDFs of MTG proxy decks.
#[derive(Parser)]
enum Args {
    /// MTG seems to have a de-facto txt format for transfering deck info.
    /// This takes info in the form of a Manabox txt export and converts it
    /// into a CSV with image URLs. Image URLs are retrieved from scryfall.
    TxtToCsv {
        input_txt_path: PathBuf,
        output_csv_path: PathBuf,
    },
    /// Takes a CSV (see `example/ninja.csv`) and generates a PDF
    /// of proxy cards from it. This uses the output of `TxtToCsv`.
    ///
    /// PNG images are not yet supported. All URLs should point
    /// to JPEGs.
    CsvToPdf {
        input_csv_path: PathBuf,
        output_pdf_path: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    match Args::parse() {
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
