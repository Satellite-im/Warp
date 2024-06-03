use crate::constellation::{self, file::Hash, Constellation};
use crate::js_exports::stream::AsyncIterator;
use futures::StreamExt;
use uuid::Uuid;
use wasm_bindgen::prelude::*;

#[derive(Clone)]
#[wasm_bindgen]
pub struct ConstellationBox {
    inner: Box<dyn Constellation>,
}
impl ConstellationBox {
    pub fn new(constellation: Box<dyn Constellation>) -> Self {
        Self {
            inner: constellation,
        }
    }
}

/// impl Constellation trait
#[wasm_bindgen]
impl ConstellationBox {
    pub fn modified(&self) -> js_sys::Date {
        self.inner.modified().into()
    }

    pub fn root_directory(&self) -> Directory {
        Directory {
            inner: self.inner.root_directory(),
        }
    }

    pub fn max_size(&self) -> usize {
        self.inner.max_size()
    }

    pub async fn put_buffer(&mut self, name: &str, buffer: &[u8]) -> Result<(), JsError> {
        self.inner
            .put_buffer(name, buffer)
            .await
            .map_err(|e| e.into())
    }

    pub async fn get_buffer(&self, name: &str) -> Result<JsValue, JsError> {
        self.inner
            .get_buffer(name)
            .await
            .map_err(|e| e.into())
            .map(|ok| serde_wasm_bindgen::to_value(&ok).unwrap())
    }

    // pub async fn put_stream(&mut self, name: &str, total_size: Option<usize>, stream: BoxStream<'static, Vec<u8>>) -> Result<ConstellationProgressStream, JsError> {
    //     self.inner.put_stream(name, total_size, stream).await.map_err(|e| e.into())
    // }

    pub async fn get_stream(&self, name: &str) -> Result<AsyncIterator, JsError> {
        self.inner
            .get_stream(name)
            .await
            .map_err(|e| e.into())
            .map(|s| {
                AsyncIterator::new(Box::pin(s.map(|t| {
                    serde_wasm_bindgen::to_value(&t.map_err(|e| String::from(e.to_string())))
                        .unwrap()
                })))
            })
    }

    pub async fn remove(&mut self, name: &str, recursive: bool) -> Result<(), JsError> {
        self.inner
            .remove(name, recursive)
            .await
            .map_err(|e| e.into())
    }

    pub async fn rename(&mut self, current: &str, new: &str) -> Result<(), JsError> {
        self.inner.rename(current, new).await.map_err(|e| e.into())
    }

    pub async fn create_directory(&mut self, name: &str, recursive: bool) -> Result<(), JsError> {
        self.inner
            .create_directory(name, recursive)
            .await
            .map_err(|e| e.into())
    }

    pub async fn sync_ref(&mut self, path: &str) -> Result<(), JsError> {
        self.inner.sync_ref(path).await.map_err(|e| e.into())
    }

    pub fn set_path(&mut self, path: String) {
        self.inner.set_path(path.into())
    }

    pub fn get_path(&self) -> String {
        self.inner.get_path().to_str().unwrap().into()
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct Directory {
    inner: constellation::directory::Directory,
}
#[wasm_bindgen]
impl Directory {
    pub fn new(name: &str) -> Self {
        Directory {
            inner: constellation::directory::Directory::new(name),
        }
    }
    pub fn has_item(&self, item_name: &str) -> bool {
        self.inner.has_item(item_name)
    }
    pub fn add_file(&self, file: File) -> Result<(), JsError> {
        self.inner.add_file(file.inner).map_err(|e| e.into())
    }
    pub fn add_directory(&self, directory: Directory) -> Result<(), JsError> {
        self.inner
            .add_directory(directory.inner)
            .map_err(|e| e.into())
    }
    pub fn get_item_index(&self, item_name: &str) -> Result<usize, JsError> {
        self.inner.get_item_index(item_name).map_err(|e| e.into())
    }
    pub fn rename_item(&self, current_name: &str, new_name: &str) -> Result<(), JsError> {
        self.inner
            .rename_item(current_name, new_name)
            .map_err(|e| e.into())
    }
    pub fn remove_item(&self, item_name: &str) -> Result<Item, JsError> {
        self.inner
            .remove_item(item_name)
            .map_err(|e| e.into())
            .map(|ok| ok.into())
    }
    pub fn remove_item_from_path(&self, directory: &str, item: &str) -> Result<Item, JsError> {
        self.inner
            .remove_item_from_path(directory, item)
            .map_err(|e| e.into())
            .map(|ok| ok.into())
    }
    pub fn move_item_to(&self, child: &str, dst: &str) -> Result<(), JsError> {
        self.inner.move_item_to(child, dst).map_err(|e| e.into())
    }
    pub fn get_items(&self) -> Vec<Item> {
        self.inner
            .get_items()
            .iter()
            .map(|i| Item::from(i.clone()))
            .collect()
    }
    pub fn set_items(&self, items: Vec<Item>) {
        self.inner.set_items(
            items
                .iter()
                .map(|i| constellation::item::Item::from(i.clone()))
                .collect(),
        )
    }
    pub fn add_item(&self, item: Item) -> Result<(), JsError> {
        self.inner.add_item(item).map_err(|e| e.into())
    }
    pub fn get_item(&self, item_name: &str) -> Result<Item, JsError> {
        self.inner
            .get_item(item_name)
            .map_err(|e| e.into())
            .map(|ok| ok.into())
    }
    pub fn find_item(&self, item_name: &str) -> Result<Item, JsError> {
        self.inner
            .find_item(item_name)
            .map_err(|e| e.into())
            .map(|ok| ok.into())
    }
    pub fn find_all_items(&self, item_names: Vec<String>) -> Vec<Item> {
        self.inner
            .find_all_items(item_names)
            .iter()
            .map(|i| Item::from(i.clone()))
            .collect()
    }
    pub fn get_last_directory_from_path(&self, path: &str) -> Result<Directory, JsError> {
        self.inner
            .get_last_directory_from_path(path)
            .map_err(|e| e.into())
            .map(|ok| Directory { inner: ok })
    }
    pub fn get_item_by_path(&self, path: &str) -> Result<Item, JsError> {
        self.inner
            .get_item_by_path(path)
            .map_err(|e| e.into())
            .map(|ok| ok.into())
    }
    pub fn name(&self) -> String {
        self.inner.name()
    }
    pub fn set_name(&self, name: &str) {
        self.inner.set_name(name)
    }
    pub fn set_thumbnail_format(&self, format: JsValue) {
        self.inner
            .set_thumbnail_format(serde_wasm_bindgen::from_value(format).unwrap())
    }
    pub fn thumbnail_format(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.thumbnail_format()).unwrap()
    }
    pub fn set_thumbnail(&self, desc: &[u8]) {
        self.inner.set_thumbnail(desc)
    }
    pub fn thumbnail(&self) -> Vec<u8> {
        self.inner.thumbnail()
    }
    pub fn set_thumbnail_reference(&self, reference: &str) {
        self.inner.set_thumbnail_reference(reference)
    }
    pub fn thumbnail_reference(&self) -> Option<String> {
        self.inner.thumbnail_reference()
    }
    pub fn set_favorite(&self, fav: bool) {
        self.inner.set_favorite(fav)
    }
    pub fn favorite(&self) -> bool {
        self.inner.favorite()
    }
    pub fn description(&self) -> String {
        self.inner.description()
    }
    pub fn set_description(&self, desc: &str) {
        self.inner.set_description(desc)
    }
    pub fn size(&self) -> usize {
        self.inner.size()
    }
    pub fn set_creation(&self, creation: js_sys::Date) {
        self.inner.set_creation(creation.into())
    }
    pub fn set_modified(&self, modified: Option<js_sys::Date>) {
        self.inner.set_modified(modified.map(|d| d.into()))
    }
    pub fn path(&self) -> String {
        self.inner.path().to_string()
    }
    pub fn set_path(&mut self, new_path: &str) {
        self.inner.set_path(new_path)
    }
    pub fn id(&self) -> String {
        self.inner.id().to_string()
    }
    pub fn creation(&self) -> js_sys::Date {
        self.inner.creation().into()
    }
    pub fn modified(&self) -> js_sys::Date {
        self.inner.modified().into()
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct File {
    inner: constellation::file::File,
}
#[wasm_bindgen]
impl File {
    pub fn new(name: &str) -> File {
        File {
            inner: constellation::file::File::new(name),
        }
    }
    pub fn name(&self) -> String {
        self.inner.name()
    }
    pub fn set_id(&self, id: String) {
        self.inner.set_id(Uuid::parse_str(&id).unwrap())
    }
    pub fn set_name(&self, name: &str) {
        self.inner.set_name(name)
    }
    pub fn description(&self) -> String {
        self.inner.description()
    }
    pub fn set_description(&self, desc: &str) {
        self.inner.set_description(desc)
    }
    pub fn set_thumbnail_format(&self, format: JsValue) {
        self.inner
            .set_thumbnail_format(serde_wasm_bindgen::from_value(format).unwrap())
    }
    pub fn thumbnail_format(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.thumbnail_format()).unwrap()
    }
    pub fn set_thumbnail(&self, data: &[u8]) {
        self.inner.set_thumbnail(data)
    }
    pub fn thumbnail(&self) -> Vec<u8> {
        self.inner.thumbnail()
    }
    pub fn set_favorite(&self, fav: bool) {
        self.inner.set_favorite(fav)
    }
    pub fn favorite(&self) -> bool {
        self.inner.favorite()
    }
    pub fn set_reference(&self, reference: &str) {
        self.inner.set_reference(reference)
    }
    pub fn set_thumbnail_reference(&self, reference: &str) {
        self.inner.set_thumbnail_reference(reference)
    }
    pub fn reference(&self) -> Option<String> {
        self.inner.reference()
    }
    pub fn thumbnail_reference(&self) -> Option<String> {
        self.inner.thumbnail_reference()
    }
    pub fn size(&self) -> usize {
        self.inner.size()
    }
    pub fn set_size(&self, size: usize) {
        self.inner.set_size(size)
    }
    pub fn set_creation(&self, creation: js_sys::Date) {
        self.inner.set_creation(creation.into())
    }
    pub fn set_modified(&self, modified: Option<js_sys::Date>) {
        self.inner.set_modified(modified.map(|d| d.into()))
    }
    pub fn hash(&self) -> Hash {
        self.inner.hash()
    }
    pub fn set_hash(&self, hash: Hash) {
        self.inner.set_hash(hash)
    }
    pub fn set_file_type(&self, file_type: JsValue) {
        self.inner
            .set_file_type(serde_wasm_bindgen::from_value(file_type).unwrap())
    }
    pub fn file_type(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.file_type()).unwrap()
    }
    pub fn path(&self) -> String {
        self.inner.path().to_string()
    }
    pub fn set_path(&mut self, new_path: &str) {
        self.inner.set_path(new_path)
    }
    pub fn id(&self) -> String {
        self.inner.id().to_string()
    }
    pub fn creation(&self) -> js_sys::Date {
        self.inner.creation().into()
    }
    pub fn modified(&self) -> js_sys::Date {
        self.inner.modified().into()
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct Item {
    inner: constellation::item::Item,
}
#[wasm_bindgen]
impl Item {
    pub fn file(&self) -> Option<File> {
        match &self.inner {
            constellation::item::Item::File(file) => Some(File {
                inner: file.clone(),
            }),
            constellation::item::Item::Directory(_) => None,
        }
    }
    pub fn directory(&self) -> Option<Directory> {
        match &self.inner {
            constellation::item::Item::File(_) => None,
            constellation::item::Item::Directory(dir) => Some(Directory { inner: dir.clone() }),
        }
    }
}
impl From<constellation::item::Item> for Item {
    fn from(value: constellation::item::Item) -> Self {
        Self { inner: value }
    }
}
impl From<Item> for constellation::item::Item {
    fn from(value: Item) -> Self {
        value.inner
    }
}
