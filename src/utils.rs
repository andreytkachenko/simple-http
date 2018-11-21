pub fn iter_fn<I, F>(cb: F) -> IterFn<I, F>
    where F: FnMut() -> Option<I>
{
    IterFn { cb }
}

pub struct IterFn<I, F>
    where F: FnMut() -> Option<I>
{
    cb: F,
}

impl<I, F> Iterator for IterFn<I, F>
    where F: FnMut() -> Option<I>
{
    type Item = I;

    fn next(&mut self) -> Option<I> {
        (self.cb)()
    }
}
