// Import our map and utility modules
use crate::map::*;   // Heightmap and Colormap traits and implementations
use crate::util::*;  // Generation options and utility functions
// Import Brickadia save file structures
use brickadia::save::{Brick, BrickColor, Collision, Color, Size};
// Import logging for progress updates
use log::info;
// Import standard library items
use std::{
    cmp::{max, min},      // For finding minimum and maximum values
    collections::HashSet, // For storing unique neighbor height values
};

/// Represents a single tile in the quadtree optimization structure
/// Tiles can be merged with adjacent similar tiles to reduce brick count
#[derive(Debug, Default)]
struct Tile {
    /// Unique index of this tile in the tiles array
    index: usize,
    /// Center position (x, y) of this tile in the heightmap
    center: (u32, u32),
    /// Size (width, height) of this tile in heightmap units
    /// Can be larger than (1,1) if tiles have been merged
    size: (u32, u32),
    /// RGBA color values for this tile [r, g, b, a]
    color: [u8; 4],
    /// Height value for this tile (elevation)
    height: u32,
    /// Set of height values from neighboring tiles
    /// Used to calculate relative height differences for brick sizing
    neighbors: HashSet<u32>,
    /// Index of parent tile if this tile has been merged into another
    /// None if this tile is still active (not merged)
    parent: Option<usize>,
}

/// QuadTree structure for optimizing brick placement
/// Contains a grid of tiles that can be merged to reduce total brick count
pub struct QuadTree {
    /// Boxed array of all tiles in the quadtree
    /// Using Box<[Tile]> for memory efficiency with large grids
    tiles: Box<[Tile]>,
    /// Width of the original heightmap/grid
    width: u32,
    /// Height of the original heightmap/grid
    height: u32,
}

impl Tile {
    /// Check if another tile is similar enough to be merged in quadtree optimization
    /// Tiles must have identical size, color, height, and both must be unmarged (no parent)
    fn similar_quad(&self, other: &Self) -> bool {
        self.size == other.size           // Same dimensions
            && self.color == other.color  // Same RGBA color
            && self.height == other.height // Same elevation
            && self.parent.is_none()      // This tile not already merged
            && other.parent.is_none()     // Other tile not already merged
    }

    /// Check if another tile can be merged in a line (horizontal or vertical)
    /// Tiles must be aligned and have matching color/height, but can differ in one dimension
    fn similar_line(&self, other: &Self) -> bool {
        let is_vertical = self.center.0 == other.center.0;   // Same X coordinate
        let is_horizontal = self.center.1 == other.center.1; // Same Y coordinate

        // Must be aligned in one direction and have matching size in the other direction
        (is_vertical && self.size.0 == other.size.0 || is_horizontal && self.size.1 == other.size.1)
            && self.color == other.color  // Same RGBA color
            && self.height == other.height // Same elevation
            && self.parent.is_none()      // This tile not already merged
            && other.parent.is_none()     // Other tile not already merged
    }

    /// Merge four adjacent tiles into this tile for quadtree optimization
    /// This creates a single larger tile that replaces four smaller ones
    fn merge_quad(
        &mut self,
        top_right: &mut Self,
        bottom_left: &mut Self,
        bottom_right: &mut Self,
    ) {
        // Double the size since we're merging 4 tiles into 1
        self.size = (self.size.0 * 2, self.size.1 * 2);

        // Combine neighbor height sets from all merged tiles
        // This preserves information about surrounding heights for brick sizing
        self.neighbors.extend(&top_right.neighbors);
        self.neighbors.extend(&bottom_left.neighbors);
        self.neighbors.extend(&bottom_right.neighbors);

        // Mark the merged tiles as children of this tile
        top_right.parent = Some(self.index);
        bottom_left.parent = Some(self.index);
        bottom_right.parent = Some(self.index);
    }
}

impl QuadTree {
    /// Create a new quadtree from heightmap and colormap data
    /// Initializes a grid of tiles, one per pixel in the input images
    /// 
    /// # Arguments
    /// * `heightmap` - Source of elevation data
    /// * `colormap` - Source of color data
    /// 
    /// # Returns
    /// * `Ok(QuadTree)` if images have matching dimensions
    /// * `Err(String)` if dimensions don't match
    pub fn new(heightmap: &dyn Heightmap, colormap: &dyn Colormap) -> Result<Self, String> {
        let (width, height) = heightmap.size();

        // Validate that both input images have matching dimensions
        if colormap.size() != heightmap.size() {
            return Err("Heightmap and colormap must have same dimensions".to_string());
        }

        // Pre-allocate vector with exact capacity for efficiency
        let mut tiles = Vec::with_capacity((width * height) as usize);

        // Create one tile for each pixel in the heightmap
        // Using i32 for loop variables to allow negative values in neighbor calculations
        for x in 0..width as i32 {
            for y in 0..height as i32 {
                tiles.push(Tile {
                    // Calculate unique index for this tile in the flattened grid
                    index: (x + y * height as i32) as usize,
                    // Store the center coordinates of this tile
                    center: (x as u32, y as u32),
                    // Collect height values from all valid neighboring pixels
                    // These are used later to calculate relative height differences
                    neighbors: vec![(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                        .into_iter()
                        // Filter out neighbors that are outside the image bounds
                        .filter(|(x, y)| {
                            *x >= 0 && *x < width as i32 && *y >= 0 && *y < height as i32
                        })
                        // Get height value for each valid neighbor
                        .map(|(x, y)| heightmap.at(x as u32, y as u32))
                        // Collect unique height values into a HashSet
                        .fold(HashSet::new(), |mut set, height| {
                            set.insert(height);
                            set
                        }),
                    // Start with size 1x1 (single pixel)
                    size: (1, 1),
                    // Get color from colormap at this position
                    color: colormap.at(x as u32, y as u32),
                    // Get elevation from heightmap at this position
                    height: heightmap.at(x as u32, y as u32),
                    // Initially no parent (not merged)
                    parent: None,
                })
            }
        }

        // Convert vector to boxed slice for memory efficiency and immutability
        Ok(QuadTree {
            tiles: tiles.into_boxed_slice(),
            width,
            height,
        })
    }

    /// Convert 2D coordinates to a linear index in the tiles array
    /// Uses column-major order: index = y + x * height
    fn index(&self, x: u32, y: u32) -> usize {
        (y + x * self.height) as usize
    }

    /// Perform quadtree optimization at a specific level
    /// Attempts to merge 2x2 groups of tiles at the given scale level
    /// 
    /// # Arguments
    /// * `level` - The scale level (0 = 1x1, 1 = 2x2, 2 = 4x4, etc.)
    /// 
    /// # Returns
    /// * Number of tiles that were successfully merged
    pub fn quad_optimize_level(&mut self, level: u32) -> usize {
        let mut count = 0;

        // Calculate spacing and step amounts for this level
        let space = 2_u32.pow(level);        // Size of tiles at this level (1, 2, 4, 8, ...)
        let step_amt = space as usize * 2;   // Step between tile groups (skip already merged tiles)

        // Iterate through the grid in steps, checking 2x2 tile groups for merging
        for x in (0..self.width - space).step_by(step_amt) {
            for y in (0..self.height - space).step_by(step_amt) {
                // Use complex array slicing to get mutable references to 4 adjacent tiles
                // This is needed because Rust's borrow checker doesn't allow multiple
                // mutable references to the same array normally
                
                // Split the tiles array vertically at x+space boundary
                let (left, right) = self
                    .tiles
                    .split_at_mut(((x + space) * self.height) as usize);

                // Split left and right columns horizontally at y+space boundary  
                let (top_left, bottom_left) =
                    left.split_at_mut((y + space + x * self.height) as usize);
                let (top_right, bottom_right) = right.split_at_mut((y + space) as usize);

                // Extract the specific tile we want from each slice
                let top_left = &mut top_left[(y + x * self.height) as usize];
                let bottom_left = &mut bottom_left[0];
                let top_right = &mut top_right[y as usize];
                let bottom_right = &mut bottom_right[0];

                // Check if all 4 tiles can be merged together
                // They must all be the same size and have matching properties
                if top_left.size.0 != space
                    || !top_left.similar_quad(top_right)
                    || !top_left.similar_quad(bottom_left)
                    || !top_left.similar_quad(bottom_right)
                {
                    continue; // Skip this group if tiles can't be merged
                }

                // Count 3 tiles eliminated (4 tiles become 1, net reduction of 3)
                count += 3;

                // Perform the merge, combining all 4 tiles into the top-left tile
                top_left.merge_quad(top_right, bottom_left, bottom_right);
            }
        }

        count
    }

    /// Merge tiles that are arranged in a horizontal or vertical line
    /// This optimization reduces brick count by combining adjacent similar tiles
    /// 
    /// # Arguments
    /// * `start_i` - Index of the first tile in the line (becomes the parent)
    /// * `children` - Vector of indices of tiles to merge into the first tile
    fn merge_line(&mut self, start_i: usize, children: Vec<usize>) {
        // Early return if no tiles to merge
        if children.is_empty() {
            return;
        }

        // Collect neighbor sets from all tiles being merged
        let mut new_neighbors = vec![];

        // Determine if this is a vertical or horizontal line merge
        // Vertical: same X coordinate (tiles stacked vertically)
        // Horizontal: same Y coordinate (tiles arranged horizontally) 
        let is_vertical = self.tiles[children[0]].center.0 == self.tiles[start_i].center.0;

        // Process each child tile: set parent and accumulate size
        let new_size = children.iter().fold(0, |sum, &i| {
            let mut t = &mut self.tiles[i];
            // Mark this tile as merged into the parent
            t.parent = Some(start_i);
            // Collect neighbor heights for the parent tile
            new_neighbors.push(t.neighbors.clone());

            // Add this tile's size to the total in the merge direction
            sum + if is_vertical { t.size.1 } else { t.size.0 }
        });

        // Update the parent tile with merged information
        let mut start = &mut self.tiles[start_i];

        // Combine neighbor height sets from all merged tiles
        for n in new_neighbors {
            start.neighbors.extend(&n);
        }

        // Increase the parent tile's size in the appropriate dimension
        if is_vertical {
            start.size.1 += new_size  // Extend height for vertical merge
        } else {
            start.size.0 += new_size  // Extend width for horizontal merge
        }
    }

    /// Optimize the quadtree by merging tiles arranged in lines
    /// This finds and merges adjacent tiles with similar properties in horizontal/vertical lines
    /// 
    /// # Arguments  
    /// * `tile_scale` - Scale factor for tile sizing (used to enforce size limits)
    /// 
    /// # Returns
    /// * Number of tiles that were merged
    pub fn line_optimize(&mut self, tile_scale: u32) -> usize {
        let mut count = 0;
        // Check every tile in the grid as a potential start of a line merge
        for x in 0..self.width {
            for y in 0..self.height {
                let start_i = self.index(x, y);
                let start = &self.tiles[start_i];
                // Skip tiles that have already been merged into other tiles
                if start.parent.is_some() {
                    continue;
                }

                // Get the current tile size for calculating merge boundaries
                let shift = start.size;
                let mut sx = shift.0;        // Current width for horizontal merging
                let mut horiz_tiles = vec![]; // Tiles to merge horizontally
                let mut sy = shift.1;        // Current height for vertical merging  
                let mut vert_tiles = vec![]; // Tiles to merge vertically

                // Find the longest possible horizontal merge from this position
                while x + sx < self.width {
                    let i = self.index(x + sx, y);
                    let t = &self.tiles[i];
                    // Stop if the resulting brick would be too large or tiles aren't similar
                    if (sx + t.size.0) * tile_scale > 500 || !start.similar_line(t) {
                        break;
                    }
                    horiz_tiles.push(i);
                    sx += t.size.0;  // Extend the total width
                }

                // Find the longest possible vertical merge from this position
                while y + sy < self.height {
                    let i = self.index(x, y + sy);
                    let t = &self.tiles[i];
                    // Stop if the resulting brick would be too large or tiles aren't similar
                    if (sy + t.size.1) * tile_scale > 500 || !start.similar_line(t) {
                        break;
                    }
                    vert_tiles.push(i);
                    sy += t.size.1;  // Extend the total height
                }

                // Count the number of tiles we'll merge (choose the longer line)
                count += max(horiz_tiles.len(), vert_tiles.len());

                // Perform the merge for whichever direction gives more reduction
                self.merge_line(
                    start_i,
                    if horiz_tiles.len() > vert_tiles.len() {
                        horiz_tiles  // Merge horizontally if it's longer
                    } else {
                        vert_tiles   // Otherwise merge vertically
                    },
                );
            }
        }

        count
    }

    /// Convert the optimized quadtree into a vector of Brickadia bricks
    /// This is the final step that creates the actual brick objects for the save file
    /// 
    /// # Arguments
    /// * `options` - Generation options controlling brick properties
    /// 
    /// # Returns
    /// * Vector of Brick objects ready for writing to a save file
    pub fn into_bricks(&self, options: GenOptions) -> Vec<Brick> {
        self.tiles
            .iter()
            .flat_map(|t| {
                // Skip tiles that have been merged or should be culled
                if t.parent.is_some()  // Skip merged tiles
                    || options.cull && (t.height == 0 || t.color[3] == 0)  // Skip transparent/ground tiles if culling enabled
                {
                    return vec![];
                }

                // Calculate the Z position (vertical placement) of this brick
                let mut z = (options.scale * t.height) as i32;

                // Calculate brick height based on height difference with neighbors
                // This creates natural-looking terrain with varying brick heights
                let raw_height = max(
                    t.height as i32 - t.neighbors.iter().cloned().min().unwrap_or(0) as i32 + 1,
                    2,  // Minimum height of 2 units
                );
                // Apply scaling and ensure minimum height
                let mut desired_height = max(raw_height * options.scale as i32 / 2, 2);

                // Snap brick positions and heights to the Brickadia grid if enabled
                // Brickadia uses a 4-unit grid system for precise brick alignment
                if options.snap {
                    z += 4 - z % 4;                    // Round Z position up to next grid line
                    desired_height += 4 - desired_height % 4;  // Round height up to grid multiple
                }

                let mut bricks = vec![];
                // Create multiple bricks if needed to reach the desired height
                // Brickadia has a maximum brick height of 250 units
                while desired_height > 0 {
                    // Calculate height for this individual brick

                    // Enforce minimum and maximum height constraints
                    let height =
                        min(max(desired_height, if options.stud { 5 } else { 2 }), 250) as u32;
                    // Ensure height is a multiple of the minimum unit (5 for studs, 2 for regular)
                    let height = height + height % (if options.stud { 5 } else { 2 });

                    // Create a new brick with the calculated properties
                    bricks.push(Brick {
                        // Reference to the brick asset type (cube, tile, micro, stud)
                        asset_name_index: options.asset,
                        // Set brick dimensions (width, depth, height)
                        size: Size::Procedural(
                            t.size.0 * options.size,  // Width based on tile size
                            t.size.1 * options.size,  // Depth based on tile size
                            // For micro brick images, use uniform cube size
                            if options.img && options.micro {
                                options.size
                            } else {
                                height  // Otherwise use calculated height
                            },
                        ),
                        // Calculate brick position in 3D space
                        position: (
                            ((t.center.0 * 2 + t.size.0) * options.size) as i32,  // X position (centered on tile)
                            ((t.center.1 * 2 + t.size.1) * options.size) as i32,  // Y position (centered on tile)
                            z - height as i32 + 2,  // Z position (bottom of brick at terrain level)
                        ),
                        // Set collision properties based on options
                        collision: Collision {
                            player: !options.nocollide,      // Player collision enabled unless disabled
                            weapon: !options.nocollide,      // Weapon collision enabled unless disabled
                            interaction: !options.nocollide, // Interaction enabled unless disabled
                            tool: true,                       // Always allow tool interaction
                        },
                        // Set brick color from the colormap
                        color: BrickColor::Unique(Color {
                            r: t.color[0],  // Red channel
                            g: t.color[1],  // Green channel
                            b: t.color[2],  // Blue channel  
                            a: t.color[3],  // Alpha (transparency)
                        }),
                        owner_index: 1,  // Reference to owner in the save file
                        material_intensity: 0,  // No special material effects
                        material_index: u32::from(options.glow),  // Glow material if enabled
                        ..Default::default()  // Use default values for remaining fields
                    });

                    // Update for next brick iteration
                    desired_height -= height as i32;  // Reduce remaining height needed
                    z -= height as i32 * 2;          // Move Z position down for next brick
                }
                bricks  // Return all bricks created for this tile
            })
            .collect()  // Flatten all brick vectors into a single vector
    }
}

/// Generate an optimized brick heightmap with quadtree and line optimizations
/// This is the main function that orchestrates the entire brick generation process
/// 
/// # Arguments
/// * `heightmap` - Source of elevation data
/// * `colormap` - Source of color data  
/// * `options` - Configuration options for brick generation
/// * `progress_f` - Callback function for progress reporting (returns true to continue)
/// 
/// # Returns
/// * `Ok(Vec<Brick>)` - Vector of optimized bricks ready for save file
/// * `Err(String)` - Error message if generation fails or is cancelled
pub fn gen_opt_heightmap<F: Fn(f32) -> bool>(
    heightmap: &dyn Heightmap,
    colormap: &dyn Colormap,
    options: GenOptions,
    progress_f: F,
) -> Result<Vec<Brick>, String> {
    // Define a macro for progress reporting with early termination
    // This allows the generation to be cancelled by returning false from progress_f
    macro_rules! progress {
        ($e:expr) => {
            if !progress_f($e) {
                return Err("Stopped by user".to_string());
            }
        };
    }
    progress!(0.0);  // Report 0% progress at start

    info!("Building initial quadtree");
    let (width, height) = heightmap.size();
    let area = width * height;  // Total number of pixels/potential bricks
    let mut quad = QuadTree::new(heightmap, colormap)?;  // Create initial 1:1 tile grid
    progress!(0.2);  // Report 20% progress after quadtree initialization

    // Determine progress tracking based on whether quadtree optimization is enabled
    let (prog_offset, prog_scale) = if options.quadtree {
        info!("Optimizing quadtree");
        let mut scale = 0;  // Start with 1x1 tiles, scale up to 2x2, 4x4, etc.

        // Perform quadtree optimization at increasing scales
        // Stop when bricks would exceed Brickadia's 500-unit size limit
        while 2_i32.pow(scale + 1) * (options.size as i32) < 500 {
            // Report progress proportional to scale level
            progress!(0.2 + 0.5 * (scale as f32 / (500.0 / (options.size as f32)).log2()));
            let count = quad.quad_optimize_level(scale);
            if count == 0 {
                break;  // No more tiles merged at this scale
            } else {
                info!("  Removed {:?} {}x bricks", count, 2_i32.pow(scale));
            }
            scale += 1;  // Move to next scale level (2x2 -> 4x4 -> 8x8, etc.)
        }
        progress!(0.7);  // 70% complete after quadtree optimization

        (0.7, 0.25)  // Remaining work starts at 70%, uses 25% of progress bar
    } else {
        (0.2, 0.75)  // Skip quadtree, remaining work starts at 20%, uses 75% of progress bar
    };

    // Perform line optimization to merge adjacent similar tiles
    info!("Optimizing linear");
    let mut i = 0;
    loop {
        i += 1;

        let count = quad.line_optimize(options.size);
        // Update progress, capping at 100% after 5 iterations
        progress!(prog_offset + prog_scale * (i as f32 / 5.0).min(1.0));

        if count == 0 {
            break;  // No more tiles merged, optimization complete
        }
        info!("  Removed {} bricks", count);
    }

    progress!(0.95);  // 95% complete before final brick generation

    // Convert the optimized quadtree into actual Brickadia bricks
    let bricks = quad.into_bricks(options);
    let brick_count = bricks.len();
    
    // Report optimization results
    info!(
        "Reduced {} to {} ({}%; -{} bricks)",
        area,                                                              // Original pixel count
        brick_count,                                                      // Final brick count
        (100. - brick_count as f64 / area as f64 * 100.).floor(),       // Reduction percentage
        area as i32 - brick_count as i32,                               // Number of bricks saved
    );

    progress!(1.0);  // 100% complete
    Ok(bricks)       // Return the final optimized brick list
}
