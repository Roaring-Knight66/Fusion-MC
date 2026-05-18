use std::error::Error;
use crate::minecraft::version::loaders::utils::assets::Assets;
use crate::minecraft::version::loaders::utils::librairies::Libraries;
use crate::minecraft::version::loaders::utils::natives::Natives;
use crate::types::VersionInfo;
use lighty_core::hosts::HTTP_CLIENT;
use tokio::{fs as async_fs, fs};
use tokio::io::AsyncWriteExt;
use lighty_core::mkdir;

use log::error;

pub trait OptifineLoader<'a> {
 
}


impl<'a> OptifineLoader<'a> for Version<'a> {
   

}