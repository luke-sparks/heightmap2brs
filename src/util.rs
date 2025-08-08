// Import Brickadia save file structures and related types
use brickadia::save::{Brick, BrickOwner, Header1, Header2, SaveData, User};
// Import standard library items for file path handling
use std::ffi::OsStr;  // OS-specific string slice for file extensions
use std::path::Path;  // Cross-platform file path handling
// Import UUID generation and parsing
use uuid::Uuid;

/// Configuration options for heightmap to brick generation
/// This struct contains all the settings that control how bricks are created
pub struct GenOptions {
    /// Size of each brick in Brickadia units (typically 5 units per stud)
    pub size: u32,
    /// Vertical scale multiplier for height values
    pub scale: u32,
    /// Index of the brick asset to use (0=cube, 1=tile, 2=micro, 3=stud)
    pub asset: u32,
    /// Whether to automatically remove bottom-level and transparent bricks
    pub cull: bool,
    /// Whether to use tile-shaped bricks instead of cubes
    pub tile: bool,
    /// Whether to use micro bricks (1/5 scale)
    pub micro: bool,
    /// Whether to use studded cube bricks
    pub stud: bool,
    /// Whether to snap brick positions to Brickadia's grid system
    pub snap: bool,
    /// Whether to create a flat image (no height variation)
    pub img: bool,
    /// Whether to make bricks glow with 0 intensity
    pub glow: bool,
    /// Whether heightmap uses high-detail RGBA encoding
    pub hdmap: bool,
    /// Whether input colors are in linear RGB (true) or sRGB (false)
    pub lrgb: bool,
    /// Whether to disable brick collision
    pub nocollide: bool,
    /// Whether to enable quadtree optimization (recommended)
    pub quadtree: bool,
    /// Height threshold above which to generate full layers
    pub gen_full_layers_above_height: u32,
}

/// Convert a single color channel from sRGB gamma to linear gamma
/// This implements the standard sRGB to linear RGB conversion formula
/// 
/// # Arguments
/// * `c` - Color channel value in sRGB gamma space (0-255)
/// 
/// # Returns
/// * Color channel value in linear gamma space (0-255)
pub fn to_linear_gamma(c: u8) -> u8 {
    let cf = (c as f64) / 255.0;  // Normalize to 0.0-1.0 range
    (if cf > 0.04045 {
        // Apply inverse gamma curve for values above the linear threshold
        (cf / 1.055 + 0.0521327).powf(2.4) * 255.0
    } else {
        // Use linear scaling for small values to avoid numerical issues
        cf / 12.192 * 255.0
    }) as u8
}

/// Convert an RGBA color from sRGB to linear RGB color space
/// This provides more accurate color calculations for lighting and blending
/// 
/// # Arguments
/// * `rgb` - RGBA color in sRGB space [r, g, b, a] where each component is 0-255
/// 
/// # Returns
/// * RGBA color in linear RGB space [r, g, b, a] with same alpha
pub fn to_linear_rgb(rgb: [u8; 4]) -> [u8; 4] {
    [
        to_linear_gamma(rgb[0]),  // Convert red channel
        to_linear_gamma(rgb[1]),  // Convert green channel
        to_linear_gamma(rgb[2]),  // Convert blue channel
        rgb[3],                   // Alpha channel remains unchanged
    ]
}

/// Convert a vector of bricks into a complete Brickadia save file structure
/// This creates all the metadata and headers needed for a valid .brs save file
/// 
/// # Arguments
/// * `bricks` - Vector of brick objects to include in the save
/// * `owner_id` - UUID string for the brick owner (or default if invalid)
/// * `owner_name` - Display name for the brick owner
/// 
/// # Returns
/// * Complete SaveData structure ready to be written to a .brs file
#[allow(unused)]  // Allow unused warning since this may not be used in all builds
pub fn bricks_to_save(bricks: Vec<Brick>, owner_id: String, owner_name: String) -> SaveData {
    // Default UUID for cases where provided owner_id is invalid
    let default_id = Uuid::parse_str("a1b16aca-9627-4a16-a160-67fa9adbb7b6").unwrap();

    // Create the author information for the save file
    let author = User {
        id: Uuid::parse_str(&owner_id).unwrap_or(default_id),  // Use provided ID or default
        name: owner_name.clone(),
    };

    // Create brick ownership information (who owns how many bricks)
    let brick_owners = vec![BrickOwner {
        id: Uuid::parse_str(&owner_id).unwrap_or(default_id),  // Same ID as author
        name: owner_name,                                      // Same name as author
        bricks: bricks.len() as u32,                         // Total brick count
    }];

    // Construct the complete save data structure
    SaveData {
        // First header contains basic save information
        header1: Header1 {
            map: String::from("https://github.com/brickadia-community"),  // Map attribution
            author,                                                         // Author information
            description: String::from("Save generated from heightmap file"), // Save description
            ..Default::default()  // Use defaults for remaining fields
        },
        // Second header contains asset and material definitions
        header2: Header2 {
            // Define the brick assets used in this save (indices match GenOptions.asset)
            brick_assets: vec![
                String::from("PB_DefaultBrick"),     // Asset 0: Standard cube brick
                String::from("PB_DefaultTile"),      // Asset 1: Tile brick
                String::from("PB_DefaultMicroBrick"), // Asset 2: Micro brick
                String::from("PB_DefaultStudded"),   // Asset 3: Studded brick
            ],
            // Define the materials that can be applied to bricks
            materials: vec!["BMC_Plastic".into(), "BMC_Glow".into()], // 0=plastic, 1=glow
            brick_owners,  // Ownership information
            ..Default::default()  // Use defaults for remaining fields
        },
        bricks,  // The actual brick data
        ..Default::default()  // Use defaults for any remaining fields
    }
}

/// Extract the file extension from a filename or path
/// This is used to determine the file type for input validation
/// 
/// # Arguments
/// * `filename` - The filename or path to extract extension from
/// 
/// # Returns
/// * `Some(&str)` - The file extension in lowercase (without the dot)
/// * `None` - If there's no extension or it contains invalid UTF-8
/// 
/// # Examples
/// ```
/// assert_eq!(file_ext("image.png"), Some("png"));
/// assert_eq!(file_ext("path/to/file.JPG"), Some("jpg"));  
/// assert_eq!(file_ext("no_extension"), None);
/// ```
#[allow(unused)]  // Allow unused warning since this may not be used in all contexts
pub fn file_ext(filename: &str) -> Option<&str> {
    Path::new(filename)        // Create a Path from the filename
        .extension()           // Extract the extension (returns Option<&OsStr>)
        .and_then(OsStr::to_str) // Convert OsStr to &str (handles UTF-8 conversion)
}
