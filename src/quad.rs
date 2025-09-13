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
    collections::{HashMap, HashSet}, // For storing unique neighbor height values and height-color mappings
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
    /// Additional tile vectors for different height layers
    /// Each vector represents tiles at a specific height level
    height_layers: Vec<Box<[Tile]>>,
    /// Sorted heights used for layer generation
    /// Used to calculate z_adjustment for each layer
    sorted_heights: Vec<u32>,
    /// Colors that were found at height 0
    /// Used to determine height adjustment for layers
    height_0_colors: HashSet<[u8; 4]>,
    /// Mapping from height to color for filtered heights
    /// Used to determine which color corresponds to each height
    filtered_heights: HashMap<u32, [u8; 4]>,
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
    /// * `gen_full_layers_above_height` - Height threshold above which to generate full layers
    /// 
    /// # Returns
    /// * `Ok(QuadTree)` if images have matching dimensions
    /// * `Err(String)` if dimensions don't match
    pub fn new(heightmap: &dyn Heightmap, colormap: &dyn Colormap, gen_full_layers_above_height: u32) -> Result<Self, String> {
        let (width, height) = heightmap.size();

        // Validate that both input images have matching dimensions
        if colormap.size() != heightmap.size() {
            return Err("Heightmap and colormap must have same dimensions".to_string());
        }

        // First pass: collect all possible heights and their colors in the heightmap
        let mut all_heights = HashMap::new();
        let mut height_0_colors = HashSet::new();
        for x in 0..width {
            for y in 0..height {
                let height = heightmap.at(x, y);
                let color = colormap.at(x, y);
                if height == 0 {
                    height_0_colors.insert(color);
                }
                all_heights.insert(height, color);
            }
        }

        // Filter heights: keep all heights above gen_full_layers_above_height,
        // and only the highest height at or below gen_full_layers_above_height
        let filtered_heights: HashMap<u32, [u8; 4]> = if gen_full_layers_above_height > 0 {
            let mut heights_at_or_below: Vec<u32> = all_heights
                .keys()
                .cloned()
                .filter(|&h| h <= gen_full_layers_above_height)
                .collect();
            heights_at_or_below.sort();
            
            let mut result = HashMap::new();
            
            // Add all heights above the threshold
            for (&height, &color) in &all_heights {
                if height > gen_full_layers_above_height {
                    result.insert(height, color);
                }
            }
            
            // Add only the highest height at or below the threshold
            if let Some(&highest_at_or_below) = heights_at_or_below.last() {
                if let Some(&color) = all_heights.get(&highest_at_or_below) {
                    result.insert(highest_at_or_below, color);
                }
            }
            
            result
        } else {
            // If gen_full_layers_above_height is 0, keep all heights
            all_heights
        };

        if gen_full_layers_above_height > 0 && !filtered_heights.is_empty() {
            // Get minimum height from filtered_heights for capping
            let min_filtered_height = *filtered_heights.keys().min().unwrap();
            
            // Create a sorted vector of filtered heights for consistent ordering
            let mut sorted_heights: Vec<u32> = filtered_heights.keys().cloned().collect();
            sorted_heights.sort();
            
            // Create tiles vector for the first layer (capped heights)
            let mut first_layer_tiles = Vec::with_capacity((width * height) as usize);
            
            // Create one tile for each pixel in the heightmap
            // Using i32 for loop variables to allow negative values in neighbor calculations
            for x in 0..width as i32 {
                for y in 0..height as i32 {
                    let original_height = heightmap.at(x as u32, y as u32);
                    // For first layer: keep original height if it's <= min_filtered_height,
                    // otherwise cap it to min_filtered_height
                    let capped_height = if original_height > min_filtered_height {
                        min_filtered_height
                    } else {
                        original_height
                    };
                    
                    first_layer_tiles.push(Tile {
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
                        color: if capped_height == min_filtered_height {
                            filtered_heights[&min_filtered_height]
                        } else {
                            colormap.at(x as u32, y as u32)
                        },
                        // Use capped height for this layer
                        height: capped_height,
                        // Initially no parent (not merged)
                        parent: None,
                    })
                }
            }
            
            // Create additional layers for remaining heights
            let mut height_layers = Vec::new();
            for &layer_height in &sorted_heights[1..] {  // Skip first height as it's already in main tiles
                let mut layer_tiles = Vec::with_capacity((width * height) as usize);
                let layer_color = filtered_heights[&layer_height];  // Get the stored color for this height
                let is_lake_layer = height_0_colors.contains(&layer_color);

                // check layer color against ocean
                // if layer color is ocean and current color != layer color, set height to 0
                
                for x in 0..width as i32 {
                    for y in 0..height as i32 {
                        let original_height = heightmap.at(x as u32, y as u32);
                        let pixel_color = colormap.at(x as u32, y as u32);

                        // Set tile height based on whether we're working on a lake or not
                        let tile_height = if is_lake_layer {
                            if pixel_color == layer_color && original_height == layer_height {
                                layer_height
                            } else {
                                0
                            }
                        } else {
                            if original_height >= layer_height {
                                layer_height
                            } else {
                                0
                            }
                        };
                        
                        // Set tile height based on correspondence and original height
                        // let tile_height = if color_corresponds_to_height_0 && original_height >= layer_height {
                        //     layer_height
                        // } else {
                        //     0
                        // };
                        
                        layer_tiles.push(Tile {
                            // Calculate unique index for this tile in the flattened grid
                            index: (x + y * height as i32) as usize,
                            // Store the center coordinates of this tile
                            center: (x as u32, y as u32),
                            // Collect height values from all valid neighboring pixels
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
                            // Use the color that was stored for this height instead of querying colormap
                            color: layer_color,
                            // Use layer height only if original matches, otherwise 0
                            height: tile_height,
                            // Initially no parent (not merged)
                            parent: None,
                        })
                    }
                }
                height_layers.push(layer_tiles.into_boxed_slice());
            }
            
            // Convert vector to boxed slice for memory efficiency and immutability
            Ok(QuadTree {
                tiles: first_layer_tiles.into_boxed_slice(),
                height_layers,
                sorted_heights,
                height_0_colors,
                filtered_heights,
                width,
                height,
            })
        } else {
            // Original behavior when gen_full_layers_above_height is 0
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
                height_layers: Vec::new(),
                sorted_heights: Vec::new(),
                height_0_colors: HashSet::new(),
                filtered_heights: HashMap::new(),
                width,
                height,
            })
        }
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

        // Optimize main tiles vector
        count += Self::quad_optimize_tiles(&mut self.tiles, self.width, self.height, space, step_amt);

        // Optimize each height layer if they exist
        for layer in &mut self.height_layers {
            count += Self::quad_optimize_tiles(layer, self.width, self.height, space, step_amt);
        }

        count
    }

    /// Helper function to perform quadtree optimization on a specific tiles array
    /// 
    /// # Arguments
    /// * `tiles` - The tiles array to optimize (either main tiles or a height layer)
    /// * `width` - Width of the tile grid
    /// * `height` - Height of the tile grid
    /// * `space` - Size of tiles at this level
    /// * `step_amt` - Step between tile groups
    /// 
    /// # Returns
    /// * Number of tiles that were successfully merged in this array
    fn quad_optimize_tiles(tiles: &mut [Tile], width: u32, height: u32, space: u32, step_amt: usize) -> usize {
        let mut count = 0;

        // Iterate through the grid in steps, checking 2x2 tile groups for merging
        for x in (0..width - space).step_by(step_amt) {
            for y in (0..height - space).step_by(step_amt) {
                // Use complex array slicing to get mutable references to 4 adjacent tiles
                // This is needed because Rust's borrow checker doesn't allow multiple
                // mutable references to the same array normally
                
                // Split the tiles array vertically at x+space boundary
                let (left, right) = tiles
                    .split_at_mut(((x + space) * height) as usize);

                // Split left and right columns horizontally at y+space boundary  
                let (top_left, bottom_left) =
                    left.split_at_mut((y + space + x * height) as usize);
                let (top_right, bottom_right) = right.split_at_mut((y + space) as usize);

                // Extract the specific tile we want from each slice
                let top_left = &mut top_left[(y + x * height) as usize];
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
    /// * `tiles` - The tiles array to operate on
    /// * `start_i` - Index of the first tile in the line (becomes the parent)
    /// * `children` - Vector of indices of tiles to merge into the first tile
    fn merge_line(tiles: &mut [Tile], start_i: usize, children: Vec<usize>) {
        // Early return if no tiles to merge
        if children.is_empty() {
            return;
        }

        // Collect neighbor sets from all tiles being merged
        let mut new_neighbors = vec![];

        // Determine if this is a vertical or horizontal line merge
        // Vertical: same X coordinate (tiles stacked vertically)
        // Horizontal: same Y coordinate (tiles arranged horizontally) 
        let is_vertical = tiles[children[0]].center.0 == tiles[start_i].center.0;

        // Process each child tile: set parent and accumulate size
        let new_size = children.iter().fold(0, |sum, &i| {
            let t = &mut tiles[i];
            // Mark this tile as merged into the parent
            t.parent = Some(start_i);
            // Collect neighbor heights for the parent tile
            new_neighbors.push(t.neighbors.clone());

            // Add this tile's size to the total in the merge direction
            sum + if is_vertical { t.size.1 } else { t.size.0 }
        });

        // Update the parent tile with merged information
        let start = &mut tiles[start_i];

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

        // Optimize main tiles vector
        count += Self::line_optimize_tiles(&mut self.tiles, self.width, self.height, tile_scale);

        // Optimize each height layer if they exist
        for layer in &mut self.height_layers {
            count += Self::line_optimize_tiles(layer, self.width, self.height, tile_scale);
        }

        count
    }

    /// Helper function to perform line optimization on a specific tiles array
    /// 
    /// # Arguments
    /// * `tiles` - The tiles array to optimize (either main tiles or a height layer)
    /// * `width` - Width of the tile grid
    /// * `height` - Height of the tile grid
    /// * `tile_scale` - Scale factor for tile sizing (used to enforce size limits)
    /// 
    /// # Returns
    /// * Number of tiles that were merged in this array
    fn line_optimize_tiles(tiles: &mut [Tile], width: u32, height: u32, tile_scale: u32) -> usize {
        let mut count = 0;
        // Check every tile in the grid as a potential start of a line merge
        for x in 0..width {
            for y in 0..height {
                let start_i = (y + x * height) as usize;  // Calculate index inline
                let start = &tiles[start_i];
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
                while x + sx < width {
                    let i = (y + (x + sx) * height) as usize;
                    let t = &tiles[i];
                    // Stop if the resulting brick would be too large or tiles aren't similar
                    if (sx + t.size.0) * tile_scale > 500 || !start.similar_line(t) {
                        break;
                    }
                    horiz_tiles.push(i);
                    sx += t.size.0;  // Extend the total width
                }

                // Find the longest possible vertical merge from this position
                while y + sy < height {
                    let i = (y + sy + x * height) as usize;
                    let t = &tiles[i];
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
                Self::merge_line(
                    tiles,
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
        let mut all_bricks = Vec::new();
        
        // Process main tiles vector
        let main_bricks = Self::tiles_to_bricks(&self.tiles, &options, 0);
        all_bricks.extend(main_bricks);
        
        // Process each height layer vector
        for (i, layer) in self.height_layers.iter().enumerate() {
            // Calculate height_adjustment based on height_0_colors logic
            // Layer i corresponds to sorted_heights[i+1]
            let height_adjustment = if self.sorted_heights.is_empty() {
                0
            } else {
                let current_height_index = i;
                if current_height_index < self.sorted_heights.len() {
                    let current_height = self.sorted_heights[current_height_index];
                    // Check if height_0_colors contains the color for this height
                    if let Some(&color) = self.filtered_heights.get(&current_height) {
                        if self.height_0_colors.contains(&color) {
                            // If height_0_colors contains the color for sorted_heights[i], use sorted_heights[i-1]
                            if current_height_index > 0 {
                                self.sorted_heights[current_height_index - 1]
                            } else {
                                0
                            }
                        } else {
                            // Use the current height as before
                            self.sorted_heights[i]
                        }
                    } else {
                        // Fallback to previous behavior
                        self.sorted_heights[i]
                    }
                } else {
                    self.sorted_heights[i]
                }
            };
            let layer_bricks = Self::tiles_to_bricks(layer, &options, height_adjustment);
            all_bricks.extend(layer_bricks);
        }
        
        all_bricks
    }
    
    /// Helper function to convert a tiles vector into bricks
    /// 
    /// # Arguments
    /// * `tiles` - The tiles vector to convert
    /// * `options` - Generation options controlling brick properties
    /// * `height_adjustment` - Value to adjust the height of bricks by
    /// 
    /// # Returns
    /// * Vector of Brick objects created from the tiles
    fn tiles_to_bricks(tiles: &[Tile], options: &GenOptions, height_adjustment: u32) -> Vec<Brick> {
        let pos_adjust = if height_adjustment == 0 {
            0
        } else {
            4
        };

        tiles
            .iter()
            .flat_map(|t| {
                // Skip tiles that have been merged or should be culled
                if t.parent.is_some()  // Skip merged tiles
                    || options.cull && (t.color[3] == 0)  // Skip transparent tiles if culling enabled
                {
                    return vec![];
                }

                // Calculate the Z position (vertical placement) of this brick
                let mut z = if t.height == 0 {
                    0
                } else {
                    (options.scale * t.height) as i32
                };

                // Calculate brick height based on height difference with neighbors
                // This creates natural-looking terrain with varying brick heights
                let raw_height = max(
                    t.height as i32 - height_adjustment as i32 + 1,
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
                                height - pos_adjust as u32  // Otherwise use calculated height
                            },
                        ),
                        // Calculate brick position in 3D space
                        position: (
                            ((t.center.0 * 2 + t.size.0) * options.size) as i32,  // X position (centered on tile)
                            ((t.center.1 * 2 + t.size.1) * options.size) as i32,  // Y position (centered on tile)
                            z - height as i32 + pos_adjust as i32 + 4 as i32,  // Z position (bottom of brick at terrain level)
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
    let mut quad = QuadTree::new(heightmap, colormap, options.gen_full_layers_above_height)?;  // Create initial 1:1 tile grid
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
