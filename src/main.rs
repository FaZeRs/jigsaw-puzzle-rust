use image::{DynamicImage, GenericImage, GenericImageView, ImageBuffer, Luma};
use rayon::prelude::*;
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const PUZZLE_GRID_SIZE: usize = 16;
const HASH_MAGIC_NUMBER: u64 = 0x9e379967;
const IMAGE_WIDTH: u32 = 3840;
const IMAGE_HEIGHT: u32 = 2160;
const FIRST_COL_WIDTH: u32 = 240;
const FIRST_ROW_HEIGHT: u32 = 135;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Top,
    Right,
    Bottom,
}

const OFFSETS: [(i32, i32); 4] = [(-1, 0), (0, -1), (1, 0), (0, 1)];

#[derive(Debug, Clone)]
struct PuzzlePiece {
    image: DynamicImage,
    col: i32,
    row: i32,
    edge_hashes: [u64; 4],
}

fn compute_hash(edge: &image::SubImage<&image::ImageBuffer<Luma<u8>, Vec<u8>>>) -> u64 {
    let mut hash = 0u64;
    for pixel in edge.pixels() {
        hash = hash.wrapping_add((pixel.2[0] as u64 / 10).wrapping_add(HASH_MAGIC_NUMBER));
        hash = hash.wrapping_add(hash.wrapping_shl(6));
        hash = hash.wrapping_add(hash.wrapping_shr(2));
    }
    hash
}

impl PuzzlePiece {
    fn new(image: DynamicImage) -> Self {
        let (width, height) = image.dimensions();

        let mut piece = PuzzlePiece {
            image,
            col: if width == FIRST_COL_WIDTH { 0 } else { -1 },
            row: if height == FIRST_ROW_HEIGHT { 0 } else { -1 },
            edge_hashes: [0; 4],
        };
        piece.compute_edge_hashes();
        piece
    }

    fn rect(&self) -> (u32, u32, u32, u32) {
        let x = FIRST_COL_WIDTH * self.col as u32 - if self.col > 0 { 1 } else { 0 };
        let y = FIRST_ROW_HEIGHT * self.row as u32 - if self.row > 0 { 1 } else { 0 };
        (x, y, self.image.width(), self.image.height())
    }

    fn compute_edge_hashes(&mut self) {
        let gray = self.image.to_luma8();
        self.edge_hashes[0] = compute_hash(&gray.view(0, 0, 1, gray.height()));
        self.edge_hashes[1] = compute_hash(&gray.view(0, 0, gray.width(), 1));
        self.edge_hashes[2] = compute_hash(&gray.view(gray.width() - 1, 0, 1, gray.height()));
        self.edge_hashes[3] = compute_hash(&gray.view(0, gray.height() - 1, gray.width(), 1));
    }
}

fn load_puzzle<P: AsRef<Path>>(path: P) -> Result<Vec<PuzzlePiece>, Box<dyn Error + Send + Sync>> {
    std::fs::read_dir(path)?
        .par_bridge()
        .map(|entry| {
            let path = entry?.path();
            let image = image::open(path)?;
            Ok(PuzzlePiece::new(image))
        })
        .collect()
}

type HashMapType = [HashMap<u64, Vec<usize>>; 4];

fn build_hash_map(pieces: &[PuzzlePiece]) -> HashMapType {
    let mut hash_maps: HashMapType = Default::default();
    for (i, piece) in pieces.iter().enumerate() {
        for j in 0..4 {
            hash_maps[j]
                .entry(piece.edge_hashes[j])
                .or_default()
                .push(i);
        }
    }
    hash_maps
}

fn assemble_puzzle(pieces: &mut [PuzzlePiece]) {
    pieces.sort_unstable_by(|a, b| {
        if (a.col == 0 || a.row == 0) && (b.col != 0 && b.row != 0) {
            std::cmp::Ordering::Less
        } else if (b.col == 0 || b.row == 0) && (a.col != 0 && a.row != 0) {
            std::cmp::Ordering::Greater
        } else if a.col == 0 && a.row == 0 {
            std::cmp::Ordering::Less
        } else if b.col == 0 && b.row == 0 {
            std::cmp::Ordering::Greater
        } else {
            (a.col + a.row).cmp(&(b.col + b.row))
        }
    });

    let hash_maps = build_hash_map(pieces);
    let mut stack = vec![0];

    while let Some(current_index) = stack.pop() {
        let (col, row): (i32, i32) = {
            let current_piece = &pieces[current_index];
            (current_piece.col, current_piece.row)
        };
        if col == PUZZLE_GRID_SIZE as i32 - 1 && row == PUZZLE_GRID_SIZE as i32 - 1 {
            continue;
        }

        for (side, opposite_side) in [(Side::Right, Side::Left), (Side::Bottom, Side::Top)] {
            if let Some(match_index) = hash_maps[opposite_side as usize]
                .get(&pieces[current_index].edge_hashes[side as usize])
                .and_then(|matches| {
                    matches.iter().find(|&&id| {
                        id != current_index && (pieces[id].col == -1 || pieces[id].row == -1)
                    })
                })
            {
                let (col_offset, row_offset) = OFFSETS[side as usize];
                pieces[*match_index].col = col + col_offset;
                pieces[*match_index].row = row + row_offset;

                stack.push(*match_index);
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let total_timer = Instant::now();
    let mut timer = Instant::now();

    let pieces_path = Path::new("peaces");

    let mut puzzle_pieces = load_puzzle(pieces_path)?;

    println!("Load puzzle time: {}ms", timer.elapsed().as_millis());
    timer = Instant::now();

    assemble_puzzle(&mut puzzle_pieces);

    println!("Assemble puzzle time: {}ms", timer.elapsed().as_millis());
    timer = Instant::now();

    let result = Arc::new(Mutex::new(ImageBuffer::new(IMAGE_WIDTH, IMAGE_HEIGHT)));
    puzzle_pieces.par_iter().for_each(|piece| {
        if piece.col >= 0
            && piece.row >= 0
            && piece.col < PUZZLE_GRID_SIZE as i32
            && piece.row < PUZZLE_GRID_SIZE as i32
        {
            let (x, y, width, height) = piece.rect();
            let mut buffer = ImageBuffer::new(width, height);
            for (dx, dy, pixel) in piece.image.to_rgb8().enumerate_pixels() {
                buffer.put_pixel(dx, dy, *pixel);
            }

            let mut result = result.lock().unwrap();
            result.copy_from(&buffer, x, y).unwrap();
        }
    });
    let result = Arc::try_unwrap(result).unwrap().into_inner().unwrap();

    println!("Image creation time: {}ms", timer.elapsed().as_millis());
    timer = Instant::now();

    result.save("result.jpg").unwrap();

    println!("Image write time: {}ms", timer.elapsed().as_millis());
    println!("Total time: {}ms", total_timer.elapsed().as_millis());

    Ok(())
}
