//! Startup restore of the in-memory NAP-INTENT handler registry.
//!
//! `IntentProvider`'s handler registry lives only in this process's memory.
//! Until this restore existed it had exactly two writers -- `install()` and
//! `reacquire_installed_artifact()` -- and neither of them runs when the
//! runtime opens. A user who installed a handler napplet, quit the app and
//! reopened it therefore had an installation the library still listed, grants
//! the store still held, a signed manifest still declaring the archetype, and
//! nothing in the process able to resolve it: `intent.invoke` reported no
//! handler until the napplet was reinstalled, with nothing on screen to say
//! why. This module rebuilds that registry from the installations already in
//! the store, using no network and no state the runtime did not already seal.

use std::sync::Arc;

use nmp_native_runtime_app::{ExecutableArtifact, PlatformCommand};
use nmp_native_runtime_store::InstalledBuild;

use super::RuntimeController;
use crate::RuntimeCatalogFailure;

impl RuntimeController {
    /// Re-registers every installed build that declares a NAP-INTENT
    /// archetype. Called once, from `open_runtime_controller`, so that every
    /// constructor gets it.
    pub(super) fn restore_intent_handlers(&self) {
        let installed = match self.runtime_store.installed_builds() {
            Ok(installed) => installed,
            Err(error) => {
                self.record_refusal(
                    "intent-handler-restore",
                    format!(
                        "the installed library could not be read while opening the runtime, so \
                         no NAP-INTENT handler was restored: {error}"
                    ),
                );
                return;
            }
        };
        for build in installed {
            // One unreadable installation costs only its own handlers. Letting
            // it abort the loop would allow a single corrupted cache entry to
            // unregister every other napplet's archetypes -- the same silent,
            // undiagnosable failure this restore exists to end. Letting it
            // pass unrecorded would be the other half of that failure.
            if let Err(failure) = self.restore_intent_handler(&build) {
                self.record_refusal(
                    "intent-handler-restore",
                    format!(
                        "{} ({}) could not be restored as a NAP-INTENT handler, so any archetype \
                         it declares is unresolvable until it is reinstalled: {}: {}",
                        build.title,
                        build.principal.d_tag(),
                        failure.code,
                        failure.detail
                    ),
                );
            }
        }
    }

    fn restore_intent_handler(
        &self,
        installed: &InstalledBuild,
    ) -> Result<(), RuntimeCatalogFailure> {
        let principal = &installed.principal;
        if self.artifacts.lock().contains_key(principal) {
            // Already attached and registered by an install in this process.
            return Ok(());
        }
        // Reopening re-reads and re-hashes every sealed byte the build
        // declares, so it is worth doing only for builds that actually
        // declare a handler. The retained signed manifest answers that
        // without touching the blob cache -- and it is re-verified before it
        // is believed, so this filter reads authenticated tags, not metadata.
        if self
            .retained_manifest(principal, installed)?
            .archetypes()
            .next()
            .is_none()
        {
            return Ok(());
        }
        let handle = self.reopen_sealed_artifact(principal, installed)?;
        // Same order as `reacquire_installed_artifact`: a handle rebuilt from
        // the sealed cache is not trusted until `verified_installed_artifact`
        // has confirmed it still carries the capability inventory the user
        // approved at install time. A registered handler is reachable by
        // every other napplet the moment it is published, so the window
        // between publishing and validating is not merely cosmetic.
        self.verified_installed_artifact(installed, Arc::clone(&handle))?;
        self.artifacts
            .lock()
            .insert(principal.clone(), Arc::clone(&handle));
        // The registry alone is not enough: `intent_dispatch::launch_handler`
        // resolves the handler's executable out of `self.artifacts`, and
        // refuses the dispatch when it is absent. Restoring the declaration
        // without the artifact would only move the failure from "no handler"
        // to "handler launch refused".
        let executable: Arc<dyn ExecutableArtifact> = handle.clone();
        self.app.dispatch(PlatformCommand::InstallVerified {
            build: installed.clone(),
            artifact: executable,
        });
        self.register_intent_handler(principal, &handle);
        Ok(())
    }
}
