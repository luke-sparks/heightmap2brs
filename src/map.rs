// External crate imports for byte ordering and image handling
use byteorder::{BigEndian, ByteOrder}; // For reading multi-byte values from image data
use image::RgbaImage;                   // RGBA image format from the image crate
use std::result::Result;                // Standard Result type for error handling

// Import color conversion utility from our util module
use crate::util::to_linear_rgb;

/// Generic trait for heightmaps that return elevation values at specific coordinates
/// Heightmaps define the vertical structure of the terrain
pub trait Heightmap {
    /// Get the height value at the given x,y coordinates
    /// Returns a u32 representing elevation (higher values = higher terrain)
    fn at(&self, x: u32, y: u32) -> u32;
    
    /// Get the dimensions of this heightmap as (width, height)
    fn size(&self) -> (u32, u32);
}

/// Generic trait for colormaps that return RGBA colors at specific coordinates
/// Colormaps define the visual appearance of each terrain point
pub trait Colormap {
    /// Get the RGBA color at the given x,y coordinates
    /// Returns [r, g, b, a] where each component is 0-255
    fn at(&self, x: u32, y: u32) -> [u8; 4];
    
    /// Get the dimensions of this colormap as (width, height)
    fn size(&self) -> (u32, u32);
}

/// PNG-based heightmap implementation that can load multiple images
/// Supports both grayscale and RGBA-encoded heightmaps for high precision
pub struct HeightmapPNG {
    /// Vector of loaded RGBA images representing height data
    maps: Vec<RgbaImage>,
    /// Whether this heightmap uses RGBA encoding for high precision heights
    /// If true, all 4 RGBA channels encode a single 32-bit height value
    /// If false, only the red channel is used as an 8-bit height value
    rgba_encoded: bool,
}

/// Implementation of the Heightmap trait for PNG-based heightmaps
impl Heightmap for HeightmapPNG {
    fn at(&self, x: u32, y: u32) -> u32 {
        if self.rgba_encoded {
            // For high-detail heightmaps, interpret all 4 RGBA channels as a 32-bit integer
            // This allows for much more precise height values than 8-bit grayscale
            self.maps
                .iter()
                .fold(0, |sum, m| sum + BigEndian::read_u32(&m.get_pixel(x, y).0))
        } else {
            // For standard heightmaps, use only the red channel as height value
            // Sum across all input maps to allow for layered heightmaps
            self.maps
                .iter()
                .fold(0, |sum, m| sum + m.get_pixel(x, y).0[0] as u32)
        }
    }

    fn size(&self) -> (u32, u32) {
        // Return dimensions of the first map (all maps must have same dimensions)
        (self.maps[0].width(), self.maps[0].height())
    }
}

/// Implementation block for HeightmapPNG construction and validation
impl HeightmapPNG {
    /// Create a new PNG heightmap from a list of image file paths
    /// 
    /// # Arguments
    /// * `images` - Vector of file paths to PNG images
    /// * `rgba_encoded` - Whether to interpret RGBA channels as 32-bit height values
    /// 
    /// # Returns
    /// * `Ok(HeightmapPNG)` if all images loaded successfully and have matching dimensions
    /// * `Err(String)` if no images provided, files couldn't be opened, or dimensions don't match
    pub fn new(images: Vec<&str>, rgba_encoded: bool) -> Result<Self, String> {
        if images.is_empty() {
            return Err("HeightmapPNG requires at least one image".to_string());
        }

        // Load all image files into RGBA format
        let mut maps: Vec<RgbaImage> = vec![];
        for file in images {
            if let Ok(img) = image::open(file) {
                // Convert any image format to RGBA8 for consistent processing
                maps.push(img.to_rgba8());
            } else {
                return Err(format!("Could not open PNG {}", file));
            }
        }

        // Validate that all images have identical dimensions
        // This is required for proper heightmap layering and indexing
        let height = maps[0].height();
        let width = maps[0].width();
        for m in &maps {
            if m.height() != height || m.width() != width {
                return Err("Mismatched heightmap sizes".to_string());
            }
        }

        // Create and return the heightmap instance
        Ok(HeightmapPNG { maps, rgba_encoded })
    }
}

/// A completely flat heightmap with uniform elevation
/// Used for image rendering mode where no height variation is desired
pub struct HeightmapFlat {
    /// Width of the flat heightmap in pixels/studs
    width: u32,
    /// Height of the flat heightmap in pixels/studs  
    height: u32,
}

/// Implementation of the Heightmap trait for flat heightmaps
/// Returns a constant height value for all coordinates
impl Heightmap for HeightmapFlat {
    fn at(&self, _x: u32, _y: u32) -> u32 {
        1
    }

    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Implementation block for HeightmapFlat construction
impl HeightmapFlat {
    /// Create a new flat heightmap with the given dimensions
    /// 
    /// # Arguments
    /// * `(width, height)` - Tuple containing the dimensions in pixels
    /// 
    /// # Returns
    /// * `Ok(HeightmapFlat)` - Always succeeds since flat heightmaps are simple
    pub fn new((width, height): (u32, u32)) -> Result<Self, String> {
-       // return a reference to save on memory
-       Ok(HeightmapFlat { width, height })
    }
}

/// PNG-based colormap implementation for reading color data from image files
/// Supports both linear RGB and sRGB color spaces
pub struct ColormapPNG {
    /// The source RGBA image containing color data
    source: RgbaImage,
    /// Whether this colormap uses linear RGB (true) or sRGB (false) color space
    /// Linear RGB provides more accurate color blending and lighting calculations
    lrgb: bool,
}

/// Implementation of the Colormap trait for PNG-based colormaps
impl Colormap for ColormapPNG {
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        if self.lrgb {
            // Input is already in linear RGB space, use directly
            self.source.get_pixel(x, y).0
        } else {
            // Input is in sRGB space, convert to linear RGB for accurate color calculations
            to_linear_rgb(self.source.get_pixel(x, y).0)
        }
    }

    fn size(&self) -> (u32, u32) {
        // Return dimensions of the source image
        (self.source.width(), self.source.height())
    }
}

/// Implementation block for ColormapPNG construction
impl ColormapPNG {
    /// Create a new PNG colormap from an image file path
    /// 
    /// # Arguments
    /// * `file` - Path to the PNG image file
    /// * `lrgb` - Whether the input image is in linear RGB (true) or sRGB (false) color space
    /// 
    /// # Returns
    /// * `Ok(ColormapPNG)` if the image loaded successfully
    /// * `Err(String)` if the image file couldn't be opened
    pub fn new(file: &str, lrgb: bool) -> Result<Self, String> {
        if let Ok(img) = image::open(file) {
            Ok(ColormapPNG {
                // Convert any image format to RGBA8 for consistent processing
                source: img.to_rgba8(),
                lrgb,
            })
        } else {
            Err(format!("Could not open PNG {}", file))
        }
    }
}
