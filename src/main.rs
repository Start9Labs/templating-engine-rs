const BUFFER_SIZE: usize = 1024;

pub fn fill_template<R: Read, W: Write>(
    escape: &EscapePattern,
    src: &mut R,
    dst: &mut W,
) -> Result<(), failure::Error> {
    let mut buf = [0; BUFFER_SIZE];
    let mut last_ends_with_n = 0;
    while {
        let bytes = src.read(&mut buf)?;
        let mut data = &buf[..bytes];
        let mut next_ends_with_n = 0;
        for i in 1..escape.0.as_bytes().len() {
            if data.ends_with(&escape.0.as_bytes()[0..i]) {
                next_ends_with_n = i;
            }
        }
        while {
            let idx: Option<isize> = if data.starts_with(&escape.0.as_bytes()[last_ends_with_n..]) {
                Some(-(last_ends_with_n as isize))
            } else {
                std::str::from_utf8(data)?
                    .find(&escape.0.as_ref())
                    .map(|a| a as isize)
            };
            if let Some(idx) = idx {
                if idx > 0 {
                    dst.write(&data[..(idx as usize)])?;
                }
                let start = idx as usize + escape.0.len();
                let mut var_name = String::new();
                loop {
                    if let Some(end) = std::str::from_utf8(data)?.find(&escape.1.as_ref()) {
                        var_name += std::str::from_utf8(&data[start..end])?;
                        log::debug!("Replacing ENV var {}.", var_name);
                        dst.write(
                            std::env::var(&var_name)
                                .map_err(|_| Error::MissingEnvVar(var_name.clone()))?
                                .as_bytes(),
                        )?;
                        data = &data[(end + escape.1.len())..];
                        break;
                    }
                    var_name += std::str::from_utf8(&data[start..])?;
                    let bytes = src.read(&mut buf)?;
                    data = &buf[..bytes];
                }
                true
            } else {
                dst.write(data)?;
                false
            }
        } {}

        last_ends_with_n = next_ends_with_n;

        bytes == BUFFER_SIZE
    } {}
    Ok(())
}

fn main() {
    let map: HashMap
}
