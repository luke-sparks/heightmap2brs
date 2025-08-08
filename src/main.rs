// Module declarations - tell Rust about the other source files in this project
pub mod map;   // Contains heightmap and colormap data structures and image processing
pub mod quad;  // Contains quadtree optimization for reducing brick count
pub mod util;  // Contains utility functions for color conversion and save file generation

// Import all public items from our modules using wildcard imports
use crate::{map::*, quad::*, util::*};
// External crate imports for file I/O, command-line parsing, and logging
use brickadia::write::SaveWriter; // Writes Brickadia save files (.brs format)
use clap::clap_app;              // Command-line argument parsing macro
use env_logger::Builder;         // Configures logging output
use log::{error, info, LevelFilter}; // Logging macros and level filtering
use std::{boxed::Box, fs::File, io::Write}; // Standard library items for file operations

fn main() {
    // Configure logging to output info-level messages and above
    // The custom format removes timestamps and log levels for cleaner output
    Builder::new()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter(None, LevelFilter::Info)
        .init();

    // Parse command-line arguments using the clap_app! macro
    // This defines all the available flags and options for the program
    let matches = clap_app!(heightmap =>
        (version: env!("CARGO_PKG_VERSION"))  // Gets version from Cargo.toml
        (author: "github.com/Meshiest")
        (about: "Converts heightmap png files to Brickadia save files")
        // Required arguments
        (@arg INPUT: +required +multiple "Input heightmap PNG images")
        // Optional file arguments
        (@arg output: -o --output +takes_value "Output BRS file")
        (@arg colormap: -c --colormap +takes_value "Input colormap PNG image")
        // Scaling and sizing options
        (@arg vertical: -v --vertical +takes_value "Vertical scale multiplier (default 1)")
        (@arg size: -s --size +takes_value "Brick stud size (default 1)")
        // Optimization and rendering flags
        (@arg cull: --cull "Automatically remove bottom level bricks and fully transparent bricks")
        (@arg tile: --tile "Render bricks as tiles")
        (@arg micro: --micro "Render bricks as micro bricks")
        (@arg stud: --stud "Render bricks as stud cubes")
        (@arg snap: --snap "Snap bricks to the brick grid")
        // Color and display options
        (@arg lrgb: --lrgb "Use linear rgb input color instead of sRGB")
        (@arg img: -i --img "Make the heightmap flat and render an image")
        (@arg glow: --glow "Make the heightmap glow at 0 intensity")
        (@arg hdmap: --hdmap "Using a high detail rgb color encoded heightmap")
        // Physics and ownership options
        (@arg nocollide: --nocollide "Disable brick collision")
        (@arg owner_id: --owner_id  +takes_value "Set the owner id (default a1b16aca-9627-4a16-a160-67fa9adbb7b6)")
        (@arg owner: --owner +takes_value "Set the owner name (default Generator)")
    )
    .get_matches();

    // Extract file paths from command-line arguments
    let heightmap_files = matches.values_of("INPUT").unwrap().collect::<Vec<&str>>();
    // If no colormap is specified, use the first heightmap file as the colormap
    let colormap_file = matches
        .value_of("colormap")
        .unwrap_or(heightmap_files[0])
        .to_string();
    // Default output file if none specified
    let out_file = matches
        .value_of("output")
        .unwrap_or("./out.brs")
        .to_string();

    // Extract owner information for the Brickadia save file
    // Each brick in Brickadia has an owner ID and name
    let owner_id = matches
        .value_of("owner_id")
        .unwrap_or("a1b16aca-9627-4a16-a160-67fa9adbb7b6")  // Default UUID
        .to_string();
    let owner_name = matches.value_of("owner").unwrap_or("Generator").to_string();

    // Build generation options from command-line arguments
    let mut options = GenOptions {
        // Brick size in Brickadia studs (multiplied by 5 for internal units)
        size: matches
            .value_of("size")
            .unwrap_or("1")
            .parse::<u32>()
            .expect("Size must be integer")
            * 5,
        // Vertical scaling factor for height values
        scale: matches
            .value_of("vertical")
            .unwrap_or("1")
            .parse::<u32>()
            .expect("Scale must be integer"),
        // Whether to remove transparent and bottom-level bricks
        cull: matches.is_present("cull"),
        // Brick asset type (set below based on tile/micro/stud flags)
        asset: 0,
        // Brick type flags
        tile: matches.is_present("tile"),   // Use tile bricks
        micro: matches.is_present("micro"), // Use micro bricks
        stud: matches.is_present("stud"),   // Use studded bricks
        // Position and alignment options
        snap: matches.is_present("snap"),   // Snap to brick grid
        // Rendering mode flags
        img: matches.is_present("img"),     // Flat heightmap for image rendering
        glow: matches.is_present("glow"),   // Make bricks glow
        hdmap: matches.is_present("hdmap"), // High detail RGBA-encoded heightmap
        lrgb: matches.is_present("lrgb"),   // Use linear RGB instead of sRGB
        nocollide: matches.is_present("nocollide"), // Disable collision
        quadtree: true, // Always enable quadtree optimization
    };

    // Set the appropriate brick asset index based on brick type
    // Asset indices correspond to different brick types in Brickadia
    if options.tile {
        options.asset = 1  // Tile brick asset
    } else if options.micro {
        options.size /= 5; // Micro bricks are 1/5 the size of regular bricks
        options.asset = 2; // Micro brick asset
    }
    if options.stud {
        options.asset = 3  // Studded brick asset (overrides tile/micro if set)
    }

    info!("Reading image files");

    // Parse the colormap file to determine brick colors
    // The colormap provides RGB color values for each pixel position
    let colormap = match file_ext(&colormap_file.to_lowercase()) {
        Some("png") => match ColormapPNG::new(&colormap_file, options.lrgb) {
            Ok(map) => map,
            Err(err) => {
                return error!("Error reading colormap: {:?}", err);
            }
        },
        Some(ext) => {
            return error!("Unsupported colormap format '{}'", ext);
        }
        None => {
            return error!("Missing colormap format for '{}'", colormap_file);
        }
    };

    // Parse the heightmap file(s) to determine brick heights
    // Heightmaps use grayscale or RGBA values to encode elevation data
    let heightmap: Box<dyn Heightmap> =
        if heightmap_files.iter().all(|f| file_ext(f) == Some("png")) {
            if options.img {
                // Create a flat heightmap for image rendering (no height variation)
                Box::new(HeightmapFlat::new(colormap.size(), options.scale).unwrap())
            } else {
                // Load PNG heightmap(s) with optional high-detail RGBA encoding
                match HeightmapPNG::new(heightmap_files, options.hdmap) {
                    Ok(map) => Box::new(map),
                    Err(error) => {
                        return error!("Error reading heightmap: {:?}", error);
                    }
                }
            }
        } else {
            return error!("Unsupported heightmap format");
        };

    // Generate optimized bricks from the heightmap and colormap
    // The callback function |_| true means we never cancel the operation
    let bricks = gen_opt_heightmap(&*heightmap, &colormap, options, |_| true)
        .expect("error during generation");

    // Write the generated bricks to a Brickadia save file
    info!("Writing Save to {}", out_file);
    let data = bricks_to_save(bricks, owner_id, owner_name);
    SaveWriter::new(File::create(out_file).unwrap(), data)
        .write()
        .expect("Failed to write file!");
    info!("Done!");
}
