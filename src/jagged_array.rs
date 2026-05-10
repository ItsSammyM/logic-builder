/// Legnths could technically be removed and calculated, but its more time efficient to have it stored in terms of reads
/// 
/// X major
/// X = index for getting an array
/// Y = index into one of the arrays
pub struct JaggedArray<T>{
    items: Vec<T>,
    //Offsets are relative to the start of the items array, so the offsets can be read directly easier
    offsets: Vec<u32>,
    lengths: Vec<u32>,
}
impl<T> JaggedArray<T> {
    pub fn get_iter(&self, x: u32)->impl Iterator<Item = &T>{
        self.items.iter().skip(self.offsets[x as usize] as usize).take(self.lengths[x as usize] as usize)
    }
}