use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for the VM hierarchy display
#[derive(Debug, Clone, Default)]
pub struct HierarchyConfig {
    /// OS family definitions
    pub families: Vec<Family>,
    /// Subcategory definitions
    pub subcategories: Vec<Subcategory>,
    /// Compiled regex patterns for categorization
    compiled_patterns: Vec<CompiledPattern>,
}

/// An OS family (top-level grouping)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Family {
    /// Internal ID (e.g., "microsoft")
    pub id: String,
    /// Display name (e.g., "Microsoft")
    pub name: String,
    /// Icon emoji
    pub icon: String,
    /// Sort order
    pub order: i32,
}

/// Sort method for VMs within a subcategory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    /// Sort by release date (oldest first) - for version-based subcategories
    Date,
    /// Sort alphabetically by display name - for non-version subcategories
    #[default]
    Name,
}

/// A subcategory within an OS family
#[derive(Debug, Clone)]
pub struct Subcategory {
    /// Internal ID (e.g., "windows_9x")
    pub id: String,
    /// Display name (e.g., "Windows 9x")
    pub name: String,
    /// Parent family ID
    pub family: String,
    /// Sort order within family
    pub order: i32,
    /// Regex patterns to match VM IDs
    pub patterns: Vec<String>,
    /// How to sort VMs within this subcategory
    pub sort_by: SortBy,
}

/// Compiled pattern for fast matching
#[derive(Debug, Clone)]
struct CompiledPattern {
    regex: Regex,
    family_id: String,
    subcategory_id: String,
}

/// Raw TOML structure for parsing
#[derive(Debug, Deserialize)]
struct HierarchyToml {
    families: HashMap<String, FamilyToml>,
    subcategories: HashMap<String, SubcategoryToml>,
}

#[derive(Debug, Deserialize)]
struct FamilyToml {
    name: String,
    icon: String,
    order: i32,
}

#[derive(Debug, Deserialize)]
struct SubcategoryToml {
    name: String,
    family: String,
    order: i32,
    patterns: Vec<String>,
    #[serde(default)]
    sort_by: Option<String>,
}

impl HierarchyConfig {
    /// Load hierarchy from embedded TOML
    pub fn load_embedded() -> Self {
        let toml_str = include_str!("../../assets/metadata/hierarchy.toml");
        Self::parse_toml(toml_str).unwrap_or_default()
    }

    /// Load hierarchy from a file path (for runtime overrides)
    #[allow(dead_code)]
    pub fn load_from_file(path: &std::path::Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        Self::parse_toml(&content).ok()
    }

    /// Parse TOML content into HierarchyConfig
    fn parse_toml(content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let raw: HierarchyToml = toml::from_str(content)?;

        // Convert families
        let mut families: Vec<Family> = raw.families
            .into_iter()
            .map(|(id, f)| Family {
                id,
                name: f.name,
                icon: f.icon,
                order: f.order,
            })
            .collect();
        families.sort_by_key(|f| f.order);

        // Convert subcategories
        let mut subcategories: Vec<Subcategory> = raw.subcategories
            .into_iter()
            .map(|(id, s)| {
                let sort_by = match s.sort_by.as_deref() {
                    Some("date") => SortBy::Date,
                    Some("name") | None => SortBy::Name,
                    _ => SortBy::Name,
                };
                Subcategory {
                    id,
                    name: s.name,
                    family: s.family,
                    order: s.order,
                    patterns: s.patterns,
                    sort_by,
                }
            })
            .collect();
        subcategories.sort_by_key(|s| (s.family.clone(), s.order));

        // Compile patterns
        let mut compiled_patterns = Vec::new();
        for subcat in &subcategories {
            for pattern in &subcat.patterns {
                if let Ok(regex) = Regex::new(&format!("(?i){}", pattern)) {
                    compiled_patterns.push(CompiledPattern {
                        regex,
                        family_id: subcat.family.clone(),
                        subcategory_id: subcat.id.clone(),
                    });
                }
            }
        }

        Ok(Self {
            families,
            subcategories,
            compiled_patterns,
        })
    }

    /// Categorize a VM by its ID
    /// Returns (family_id, subcategory_id)
    pub fn categorize(&self, vm_id: &str) -> (String, String) {
        for cp in &self.compiled_patterns {
            if cp.regex.is_match(vm_id) {
                return (cp.family_id.clone(), cp.subcategory_id.clone());
            }
        }
        // Default to "other" family and "uncategorized" subcategory
        ("other".to_string(), "uncategorized".to_string())
    }

    /// Get a family by ID
    #[allow(dead_code)]
    pub fn get_family(&self, id: &str) -> Option<&Family> {
        self.families.iter().find(|f| f.id == id)
    }

    /// Get a subcategory by ID
    pub fn get_subcategory(&self, id: &str) -> Option<&Subcategory> {
        self.subcategories.iter().find(|s| s.id == id)
    }

    /// Get all subcategories for a family
    pub fn subcategories_for_family(&self, family_id: &str) -> Vec<&Subcategory> {
        self.subcategories
            .iter()
            .filter(|s| s.family == family_id)
            .collect()
    }
}
