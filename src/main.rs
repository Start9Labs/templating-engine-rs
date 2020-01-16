use serde_yaml::Mapping;

use tmpl::*;

fn main() -> Result<(), Error> {
    let (escape, map_file) = {
        let mut args = std::env::args();
        args.next();
        let arg = args.next().ok_or(Error::MissingArgument)?;
        if arg == "--template" || arg == "-t" {
            let escape = args.next().ok_or(Error::MissingArgument)?.parse()?;
            let map_file = args.next().ok_or(Error::MissingArgument)?;
            (escape, map_file)
        } else {
            (Default::default(), arg)
        }
    };
    let map: Mapping = serde_yaml::from_reader(std::fs::File::open(map_file)?)?;
    std::io::copy(
        &mut TemplatingReader::new(std::io::stdin(), &map, &escape, b'%'),
        &mut std::io::stdout(),
    )?;

    Ok(())
}
