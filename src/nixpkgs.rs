//! Read package information from nix-env.

use std::{path::PathBuf, pin::Pin, sync::Arc};
use anyhow::Context;
use futures::prelude::*;

use nix_bindings::{self as nix, NixValueOps, Value, ValueType::{self, Attrs, Thunk}};

thread_local! {
    pub static NIX_CONTEXT: Arc<nix::Context> = {
        let thread_id = std::thread::current().id();
        let ctx = nix::Context::new()
            .expect(format!("Could not initialise Nix library context in thread {:?}!", thread_id).as_str());

        Arc::new(ctx)
    };
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Package {
    name: String,
    version: String
}

impl<'a> TryFrom<(String, Value<'a>)> for Package {
    type Error = anyhow::Error;
    
    fn try_from((name, attrs): (String, Value<'a>)) -> anyhow::Result<Package> {
        println!("try_from({}, {:?})", name, attrs);
        #[allow(non_snake_case)]
        match attrs.value_type() {
            Thunk => {
                attrs.force().with_context(|| format!("Failed to force thunk for key '{}'", name))?;

                // If Value::force did not get rid of the Thunk, then we will go into an infinite loop here.
                // However, force *should* error in such cases instead of doing nothing.
                debug_assert_eq!(attrs.value_type(), ValueType::Thunk);

                Ok(Self::try_from((name, attrs))?)
            },
            Attrs => {
                if !attrs.has_attr("version")? {
                    return Err(nix::Error::KeyNotFound("version".to_owned()).into());
                }

                let version = attrs.get_attr("version")?.as_string()?;
                Ok(Package { name, version })
            },
            _ => Err(anyhow::Error::new(nix::Error::InvalidType { expected: "attrs", actual: attrs.type_name() }))
                .context("Tried to create package from non-attrs object")
        }
    }
}

pub struct PackagesQuery(std::sync::mpsc::Receiver<Package>);

const IMPORT_NIXPKGS: &str = "import <nixpkgs> {}";

impl PackagesQuery {
    pub async fn new(nixpkgs: PathBuf) -> anyhow::Result<PackagesQuery> {
        let (tx, rx) = std::sync::mpsc::channel::<Package>();

        tokio::task::spawn_blocking(move || {
            NIX_CONTEXT.with(|ctx| {
                let store = Arc::new(nix::Store::open(ctx, None)
                    .context("Failed to open the default Nix store")?);

                let nixpkgs = nixpkgs.canonicalize()
                    .with_context(|| format!("Could not canonicalize nixpkgs path: {:?}", nixpkgs))?;

                let paths = [format!("nixpkgs={}", nixpkgs.display())];
                let eval = nix::EvalStateBuilder::new(&store)?
                    .set_lookup_path(&paths)?
                    .build()
                    .context("Could not construct the EvalState")?;
                
                let pkgs = eval.eval_from_string(IMPORT_NIXPKGS, "<eval>")
                    .with_context(|| format!("Failed to evaluate '{}'", IMPORT_NIXPKGS))?;

                pkgs.attrs()?
                    .filter_map(Result::ok)
                    .filter_map(|attr| match attr {
                        (ref name, ref value) if value.value_type() == ValueType::Thunk => {
                            println!("forcing thunk for {}", name);
                            value.force().ok().map(|_| attr)
                        },
                        _ => Some(attr)
                    })
                    .filter(|(_, value)| value.value_type() == ValueType::Attrs)
                    .try_for_each(|attr| {
                        match Package::try_from(attr) {
                            Ok(pkg) => tx.send(pkg).map_err(anyhow::Error::new),
                            Err(_) => Ok(()), // This is not a package, skip
                        }
                    })
            })?;

            Ok(PackagesQuery(rx))
        }).await?
    }
}

impl Stream for PackagesQuery {
    type Item = anyhow::Result<Package>;
    
    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<anyhow::Result<Package>>> {
        use std::task::Poll::*;
        use std::sync::mpsc::TryRecvError::*;

        #[allow(non_snake_case)]
        match self.0.try_recv() {
            Ok(pkg) => Ready(Some(Ok(pkg))),
            Err(Empty) => Pending,
            Err(Disconnected) => Ready(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use nix_bindings::{EvalState, EvalStateBuilder, flake::{FetchersSettings, FlakeSettings, LockedFlake}};

    use super::*;

    fn create_test_eval_flake() {
        /*NIX_CONTEXT.with(|ctx: &nix::Context| {
            let flake_settings = nix::flake::FlakeSettings::new(ctx)?;
            let fetch_settings = nix::flake::FetchersSettings::new(ctx)?;
            let eval_state = nix::EvalStateBuilder::new(store)?;

            LockedFlake::lock(
                context,
                &fetch_settings,
                &flake_settings,
                eval_state,
                lock_flags,
                flake_ref
            );
        });*/
    }

    /*#[tokio::test(flavor = "multi_thread")]
    pub async fn test_query_packages() -> anyhow::Result<()> {
        use std::io::Write;
        use futures::TryStreamExt;

        let query = PackagesQuery::new(PathBuf::try_from("/nix/var/nix/profiles/per-user/root/channels/nixpkgs")?).await?;

        println!("Querying...");

        query.try_for_each_concurrent(None, async |pkg| {
            let mut stdout = std::io::stdout().lock();
            writeln!(stdout, "{} {}", pkg.name, pkg.version)?;
            Ok(())
        }).await
    }*/
}