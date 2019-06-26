pub fn time_fn<F, U>(f: F) -> (i64, U)
    where F: Fn() -> U
{
    let t0 = time::get_time();
    let res = f();
    let t1 = time::get_time();

    ((t1-t0).num_milliseconds(), res)
}
