#[cfg(feature = "dynamic_assets")]
use bevy::asset::Assets;
use bevy::asset::{AssetServer, LoadState};
use bevy::ecs::prelude::{FromWorld, State, World};
use bevy::ecs::schedule::StateData;
use std::marker::PhantomData;

#[cfg(feature = "dynamic_assets")]
use crate::dynamic_asset::DynamicAssetCollection;
#[cfg(feature = "dynamic_assets")]
use crate::AssetKeys;
use crate::{AssetCollection, AssetLoaderConfiguration, LoadingAssetHandles, LoadingStatePhase};

pub(crate) fn init_resource<Asset: FromWorld + Send + Sync + 'static>(world: &mut World) {
    let asset = Asset::from_world(world);
    world.insert_resource(asset);
}

pub(crate) fn loading_state<S: StateData, Assets: AssetCollection>(world: &mut World) {
    let phase = {
        let cell = world.cell();
        let asset_loader_configuration = cell
            .get_resource::<AssetLoaderConfiguration<S>>()
            .expect("Cannot get AssetLoaderConfiguration");
        let state = cell.get_resource::<State<S>>().expect("Cannot get state");
        asset_loader_configuration
            .phase
            .get(state.current())
            .unwrap()
            .clone()
    };

    #[allow(unreachable_patterns)]
    match phase {
        LoadingStatePhase::StartLoading => start_loading_collections::<S, Assets>(world),
        LoadingStatePhase::Loading => check_loading_state::<S, Assets>(world),
        _ => {}
    }
}

fn start_loading_collections<S: StateData, Assets: AssetCollection>(world: &mut World) {
    {
        let cell = world.cell();
        let mut asset_loader_configuration = cell
            .get_resource_mut::<AssetLoaderConfiguration<S>>()
            .expect("Cannot get AssetLoaderConfiguration");
        let state = cell.get_resource::<State<S>>().expect("Cannot get state");
        let mut config = asset_loader_configuration
            .configuration
            .get_mut(state.current())
            .unwrap_or_else(|| {
                panic!(
                    "Could not find a loading configuration for state {:?}",
                    state.current()
                )
            });
        config.count += 1;
    }
    let handles = LoadingAssetHandles {
        handles: Assets::load(world),
        marker: PhantomData::<Assets>,
    };
    world.insert_resource(handles);
}

fn check_loading_state<S: StateData, Assets: AssetCollection>(world: &mut World) {
    {
        let cell = world.cell();

        let loading_asset_handles = cell.get_resource::<LoadingAssetHandles<Assets>>();
        if loading_asset_handles.is_none() {
            return;
        }
        let loading_asset_handles = loading_asset_handles.unwrap();

        let asset_server = cell
            .get_resource::<AssetServer>()
            .expect("Cannot get AssetServer resource");
        let load_state = asset_server
            .get_group_load_state(loading_asset_handles.handles.iter().map(|handle| handle.id));
        if load_state != LoadState::Loaded {
            return;
        }

        let mut state = cell
            .get_resource_mut::<State<S>>()
            .expect("Cannot get State resource");
        let mut asset_loader_configuration = cell
            .get_resource_mut::<AssetLoaderConfiguration<S>>()
            .expect("Cannot get AssetLoaderConfiguration resource");
        if let Some(mut config) = asset_loader_configuration
            .configuration
            .get_mut(state.current())
        {
            config.count -= 1;
            if config.count == 0 {
                if let Some(next) = config.next.as_ref() {
                    state.set(next.clone()).expect("Failed to set next State");
                }
            }
        }
    }
    let asset_collection = Assets::create(world);
    world.insert_resource(asset_collection);
    world.remove_resource::<LoadingAssetHandles<Assets>>();
}

pub(crate) fn phase<S: StateData>(world: &mut World) {
    let phase = {
        let cell = world.cell();
        let asset_loader_configuration = cell
            .get_resource::<AssetLoaderConfiguration<S>>()
            .expect("Cannot get AssetLoaderConfiguration");
        let state = cell.get_resource::<State<S>>().expect("Cannot get state");
        asset_loader_configuration
            .phase
            .get(state.current())
            .unwrap()
            .clone()
    };

    match phase {
        #[cfg(feature = "dynamic_assets")]
        LoadingStatePhase::PreparingAssetKeys => {
            let cell = world.cell();
            let asset_server = cell
                .get_resource::<AssetServer>()
                .expect("Cannot get AssetServer resource");
            let mut asset_loader_configuration = cell
                .get_resource_mut::<AssetLoaderConfiguration<S>>()
                .expect("Cannot get AssetLoaderConfiguration");
            let load_state = asset_server.get_group_load_state(
                asset_loader_configuration
                    .asset_collection_handles
                    .iter()
                    .map(|handle| handle.id),
            );
            if load_state == LoadState::Loaded {
                let mut dynamic_asset_collections = cell
                    .get_resource_mut::<Assets<DynamicAssetCollection>>()
                    .expect("Cannot get AssetServer resource");
                let state = cell.get_resource::<State<S>>().expect("Cannot get state");

                let mut asset_keys = cell.get_resource_mut::<AssetKeys>().unwrap();
                // Todo: why add the manual dynamic assets to all loaded collections?
                for collection in asset_loader_configuration
                    .asset_collection_handles
                    .drain(..)
                {
                    let collection = dynamic_asset_collections.remove(collection).unwrap();
                    collection.apply(&mut asset_keys);
                }
                asset_loader_configuration
                    .phase
                    .insert(state.current().clone(), LoadingStatePhase::StartLoading);
            }
        }
        LoadingStatePhase::StartLoading => {
            let cell = world.cell();
            let mut asset_loader_configuration = cell
                .get_resource_mut::<AssetLoaderConfiguration<S>>()
                .expect("Cannot get AssetLoaderConfiguration");
            let state = cell.get_resource::<State<S>>().expect("Cannot get state");
            asset_loader_configuration
                .phase
                .insert(state.current().clone(), LoadingStatePhase::Loading);
        }
        _ => (),
    }
}
