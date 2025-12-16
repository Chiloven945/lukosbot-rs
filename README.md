<div align="middle">

# lukosbot-rs

[![Visitors](https://api.visitorbadge.io/api/visitors?path=https%3A%2F%2Fgithub.com%2FChiloven945%2Flukosbot-rs&labelColor=%23444444&countColor=%23f24822&style=flat-square&labelStyle=none)](https://visitorbadge.io/status?path=https%3A%2F%2Fgithub.com%2FChiloven945%2Flukosbot-rs)
[![Stars](https://img.shields.io/github/stars/Chiloven945/lukosbot-rs?style=flat-square&logo=data:image/svg%2bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZlcnNpb249IjEiIHdpZHRoPSIxNiIgaGVpZ2h0PSIxNiI+PHBhdGggZD0iTTggLjI1YS43NS43NSAwIDAgMSAuNjczLjQxOGwxLjg4MiAzLjgxNSA0LjIxLjYxMmEuNzUuNzUgMCAwIDEgLjQxNiAxLjI3OWwtMy4wNDYgMi45Ny43MTkgNC4xOTJhLjc1MS43NTEgMCAwIDEtMS4wODguNzkxTDggMTIuMzQ3bC0zLjc2NiAxLjk4YS43NS43NSAwIDAgMS0xLjA4OC0uNzlsLjcyLTQuMTk0TC44MTggNi4zNzRhLjc1Ljc1IDAgMCAxIC40MTYtMS4yOGw0LjIxLS42MTFMNy4zMjcuNjY4QS43NS43NSAwIDAgMSA4IC4yNVoiIGZpbGw9IiNlYWM1NGYiLz48L3N2Zz4=&logoSize=auto&label=Stars&labelColor=444444&color=eac54f)](https://github.com/Chiloven945/lukosbot-rs/)
[![GitHub CI](https://img.shields.io/github/actions/workflow/status/Chiloven945/lukosbot-rs/cargo.yml?style=flat-square&labelColor=444444&branch=master&label=GitHub%20CI&logo=github)](https://github.com/Chiloven945/lukosbot-rs/actions/workflows/cargo.yml)

</div>

lukosbot-rs is a multifunctional and multiplatform chatbot, using
the [teloxide](https://github.com/teloxide/teloxide) for Telegram platform,
the [serenity](https://github.com/serenity-rs/serenity) for Discord platform, and analysing command with
the [azalea-brigadier](https://github.com/azalea-rs/azalea/tree/main/azalea-brigadier).

This is an experimental project. I'm currently learning Rust and using it to practice my skills, and it is not
guaranteed to be stable or secure.
Please use at your own risk.

## Supported Commands

None of commands are supported, this is just a framework currently.

## Planned Features

I'm planning to migrate all the feature from the original project lukosBot2, so all the commands will be migrated and
usable in the future.

## Contributing

If you want to contribute to this project, feel free to open an issue or a pull request. Contributions are welcome!

## License

This project is licensed under the GNU Affero General Public License v3.0 (AGPL-3.0). See the [LICENSE](LICENSE) file
for details.

This project uses the following third-party libraries. All dependencies are compatible with AGPL-3.0, and their licences
are respected:

- **anyhow**
    - Repository: [dtolnay/anyhow](https://github.com/dtolnay/anyhow)
    - License: [MIT](https://github.com/dtolnay/anyhow/blob/master/LICENSE-MIT)
      and [Apache-2.0](https://github.com/dtolnay/anyhow/blob/master/LICENSE-APACHE)

- **async-trait**
    - Repository: [dtolnay/async-trait](https://github.com/dtolnay/async-trait)
    - License: [MIT](https://github.com/dtolnay/async-trait/blob/master/LICENSE-MIT)
      and [Apache-2.0](https://github.com/dtolnay/async-trait/blob/master/LICENSE-APACHE)

- **azalea-brigadier**
    - Repository: [azalea-rs/brigadier](https://github.com/azalea-rs/azalea/tree/main/azalea-brigadier)
    - License: [MIT](https://github.com/azalea-rs/azalea/blob/main/LICENSE.md)

- **regex**
    - Repository: [rust-lang/regex](https://github.com/rust-lang/regex)
    - License: [MIT](https://github.com/rust-lang/regex/blob/master/LICENSE-MIT)
      and [Apache-2.0](https://github.com/rust-lang/regex/blob/master/LICENSE-APACHE)

- **serde**
    - Repository: [serde-rs/serde](https://github.com/serde-rs/serde)
    - License: [MIT](https://github.com/serde-rs/serde/blob/master/LICENSE-MIT)
      and [Apache-2.0](https://github.com/serde-rs/serde/blob/master/LICENSE-APACHE)

- **serde_yaml**
    - Repository: [dtolnay/serde-yaml](https://github.com/dtolnay/serde-yaml)
    - License: [MIT](https://github.com/dtolnay/serde-yaml/blob/master/LICENSE-MIT)
      and [Apache-2.0](https://github.com/dtolnay/serde-yaml/blob/master/LICENSE-APACHE)

- **serenity**
    - Repository: [serenity-rs/serenity](https://github.com/serenity-rs/serenity)
    - License: [ISC](https://github.com/serenity-rs/serenity/blob/current/LICENSE.md)

- **teloxide**
    - Repository: [teloxide/teloxide](https://github.com/teloxide/teloxide)
    - License: [MIT](https://github.com/teloxide/teloxide/blob/master/LICENSE)

- **tokio**
    - Repository: [tokio-rs/tokio](https://github.com/tokio-rs/tokio)
    - License: [MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE)

- **tracing**
    - Repository: [tokio-rs/tracing](https://github.com/tokio-rs/tracing)
    - License: [MIT](https://github.com/tokio-rs/tracing/blob/master/LICENSE)

- **tracing-subscriber**
    - Repository: [tokio-rs/tracing](https://github.com/tokio-rs/tracing)
    - License: [MIT](https://github.com/tokio-rs/tracing/blob/master/LICENSE)

- **url**
    - Repository: [servo/rust-url](https://github.com/servo/rust-url)
    - License: [MIT](https://github.com/servo/rust-url/blob/main/LICENSE-MIT)
      and [Apache-2.0](https://github.com/servo/rust-url/blob/main/LICENSE-APACHE)
