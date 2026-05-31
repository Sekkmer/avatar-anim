use crate::{AnimError, Result};
use glam::Vec3;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use xml::reader::{EventReader, XmlEvent};

#[derive(Clone, Debug, PartialEq)]
pub struct SkeletonBone {
    pub name: String,
    pub pos: Vec3,
    pub parent: Option<String>,
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkeletonDefinition {
    pub bones: Vec<SkeletonBone>,
}

impl SkeletonDefinition {
    pub fn from_xml_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path).map_err(AnimError::Io)?;
        Self::from_xml_reader(BufReader::new(file))
    }

    pub fn from_xml_reader<R: Read>(reader: R) -> Result<Self> {
        let parser = EventReader::new(reader);
        let mut bones = Vec::new();
        let mut parent_stack = Vec::<Option<String>>::new();

        for event in parser {
            let event = event.map_err(|e| {
                AnimError::InvalidStructure(format!("Skeleton XML parse error: {e}"))
            })?;
            match event {
                XmlEvent::StartElement {
                    name, attributes, ..
                } if name.local_name == "bone" => {
                    let parent = parent_stack.iter().rev().find_map(Clone::clone);
                    let mut attrs = BTreeMap::new();
                    let mut bone_name = None;
                    let mut pos = None;
                    for attr in attributes {
                        let attr_name = attr.name.local_name;
                        if attr_name == "name" {
                            bone_name = Some(attr.value.clone());
                        } else if attr_name == "pos" || attr_name == "position" {
                            pos = Some(parse_vec3(&attr.value)?);
                        }
                        attrs.insert(attr_name, attr.value);
                    }

                    parent_stack.push(bone_name.clone());
                    if let Some(name) = bone_name
                        && let Some(pos) = pos
                    {
                        bones.push(SkeletonBone {
                            name,
                            pos,
                            parent,
                            attributes: attrs,
                        });
                    }
                }
                XmlEvent::EndElement { name } if name.local_name == "bone" => {
                    parent_stack.pop();
                }
                _ => {}
            }
        }

        Ok(Self { bones })
    }

    pub fn bone(&self, name: &str) -> Option<&SkeletonBone> {
        self.bones.iter().find(|bone| bone.name == name)
    }

    pub fn position(&self, name: &str) -> Option<Vec3> {
        self.bone(name).map(|bone| bone.pos)
    }

    pub fn bones_with_prefix(&self, prefix: &str) -> Vec<&SkeletonBone> {
        self.bones
            .iter()
            .filter(|bone| bone.name.starts_with(prefix))
            .collect()
    }

    pub fn bones_in_group(&self, group: &str) -> Vec<&SkeletonBone> {
        self.bones
            .iter()
            .filter(|bone| bone.attributes.get("group").is_some_and(|g| g == group))
            .collect()
    }
}

fn parse_vec3(value: &str) -> Result<Vec3> {
    let mut parts = value.split_whitespace();
    let x = parse_component(parts.next(), value)?;
    let y = parse_component(parts.next(), value)?;
    let z = parse_component(parts.next(), value)?;
    if parts.next().is_some() {
        return Err(AnimError::InvalidStructure(format!(
            "Expected 3-vector, got '{value}'"
        )));
    }
    Ok(Vec3::new(x, y, z))
}

fn parse_component(component: Option<&str>, original: &str) -> Result<f32> {
    component
        .ok_or_else(|| AnimError::InvalidStructure(format!("Expected 3-vector, got '{original}'")))?
        .parse::<f32>()
        .map_err(|_| {
            AnimError::InvalidStructure(format!("Invalid vector component in '{original}'"))
        })
}
