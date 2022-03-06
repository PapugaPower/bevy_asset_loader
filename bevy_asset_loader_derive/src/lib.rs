//! This crate adds support for auto deriving [`AssetCollection`](bevy_asset_loader::AssetCollection)
//!
//! You do not have to use it directly. Just import ``AssetCollection`` from ``bevy_asset_loader``
//! and use ``#[derive(AssetCollection)]`` to derive the trait.

#![forbid(unsafe_code)]
#![warn(unused_imports)]

extern crate proc_macro;

mod assets;

use proc_macro::TokenStream;
use std::option::Option::Some;
use std::result::Result::{Err, Ok};

use crate::assets::*;
use proc_macro2::Ident;
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{Data, Field, Fields, Lit, Meta, NestedMeta};

/// Derive macro for [`AssetCollection`](bevy_asset_loader::AssetCollection)
///
/// The helper attribute ``asset`` can be used to define the path to the asset file
/// and other asset options.
#[proc_macro_derive(AssetCollection, attributes(asset))]
pub fn asset_collection_derive(input: TokenStream) -> TokenStream {
    let ast = syn::parse(input).unwrap();
    impl_asset_collection(ast)
        .unwrap_or_else(to_compile_errors)
        .into()
}

pub(crate) const ASSET_ATTRIBUTE: &str = "asset";
pub(crate) const PATH_ATTRIBUTE: &str = "path";
pub(crate) const KEY_ATTRIBUTE: &str = "key";
pub(crate) const OPTIONAL_ATTRIBUTE: &str = "optional";

pub(crate) const TEXTURE_ATLAS_ATTRIBUTE: &str = "texture_atlas";
pub(crate) struct TextureAtlasAttribute;
impl TextureAtlasAttribute {
    pub const TILE_SIZE_X: &'static str = "tile_size_x";
    pub const TILE_SIZE_Y: &'static str = "tile_size_y";
    pub const COLUMNS: &'static str = "columns";
    pub const ROWS: &'static str = "rows";
    #[allow(dead_code)]
    pub const PADDING_X: &'static str = "padding_x";
    #[allow(dead_code)]
    pub const PADDING_Y: &'static str = "padding_y";
}

pub(crate) const FOLDER_ATTRIBUTE: &str = "folder";
pub(crate) const FOLDER_TYPED_ATTRIBUTE: &str = "typed";
pub(crate) const STANDARD_MATERIAL_ATTRIBUTE: &str = "standard_material";

fn impl_asset_collection(
    ast: syn::DeriveInput,
) -> Result<proc_macro2::TokenStream, Vec<syn::Error>> {
    let name = &ast.ident;

    let mut default_fields: Vec<Ident> = vec![];
    let mut assets: Vec<AssetField> = vec![];
    if let Data::Struct(ref data_struct) = ast.data {
        if let Fields::Named(ref named_fields) = data_struct.fields {
            let mut compile_errors = vec![];
            for field in named_fields.named.iter() {
                match parse_field(field) {
                    Ok(asset) => assets.push(asset),
                    Err(errors) => {
                        for error in errors {
                            match error {
                                ParseFieldError::NoAttributes => {
                                    default_fields.push(field.clone().ident.unwrap())
                                }
                                ParseFieldError::KeyAttributeStandsAlone => {
                                    compile_errors.push(syn::Error::new_spanned(
                                        field.into_token_stream(),
                                        "The 'key' attribute cannot be combined with any other asset defining attributes",
                                    ));
                                }
                                ParseFieldError::OnlyDynamicCanBeOptional => {
                                    compile_errors.push(syn::Error::new_spanned(
                                        field.into_token_stream(),
                                        "Only a dynamic asset (with 'key' attribute) can be optional",
                                    ));
                                }
                                ParseFieldError::MissingAttributes(missing_attributes) => {
                                    compile_errors.push(syn::Error::new_spanned(
                                        field.into_token_stream(),
                                        format!(
                                            "Field is missing asset attributes: {}",
                                            missing_attributes.join(", ")
                                        ),
                                    ));
                                }
                                ParseFieldError::WrongAttributeType(token_stream, expected) => {
                                    compile_errors.push(syn::Error::new_spanned(
                                        token_stream,
                                        format!("Wrong attribute type. Expected '{}'", expected),
                                    ));
                                }
                                ParseFieldError::UnknownAttributeType(token_stream) => {
                                    compile_errors.push(syn::Error::new_spanned(
                                        token_stream,
                                        "Unknown attribute type",
                                    ));
                                }
                                ParseFieldError::UnknownAttribute(token_stream) => {
                                    compile_errors.push(syn::Error::new_spanned(
                                        token_stream,
                                        "Unknown attribute",
                                    ));
                                }
                                ParseFieldError::MissingRenderFeature(token_stream) => {
                                    compile_errors.push(syn::Error::new_spanned(
                                        token_stream,
                                        "This attribute requires the 'render' feature",
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            if !compile_errors.is_empty() {
                return Err(compile_errors);
            }
        } else {
            return Err(vec![syn::Error::new_spanned(
                data_struct.fields.clone().into_token_stream(),
                "only named fields are supported to derive AssetCollection",
            )]);
        }
    } else {
        return Err(vec![syn::Error::new_spanned(
            &ast.into_token_stream(),
            "AssetCollection can only be derived for a struct",
        )]);
    }

    let asset_loading = assets.iter().fold(quote!(), |token_stream, asset| {
        asset.attach_token_stream_for_loading(token_stream)
    });
    let load_function = quote! {
            fn load(world: &mut World) -> Vec<HandleUntyped> {
                let cell = world.cell();
                let asset_server = cell.get_resource::<AssetServer>().expect("Cannot get AssetServer");
                let asset_keys = cell.get_resource::<bevy_asset_loader::AssetKeys>().expect("Cannot get bevy_asset_loader::AssetKeys");
                let mut handles = vec![];
                #asset_loading
                handles
            }
    };

    #[allow(unused_mut, unused_assignments)]
    let mut conditional_asset_collections = quote! {};
    #[cfg(feature = "render")]
    {
        // standard materials and texture atlas resources
        conditional_asset_collections = quote! {
                let mut materials = cell
                    .get_resource_mut::<Assets<StandardMaterial>>()
                    .expect("Cannot get resource Assets<StandardMaterial>");
                let mut atlases = cell
                    .get_resource_mut::<Assets<TextureAtlas>>()
                    .expect("Cannot get resource Assets<TextureAtlas>");
        };
    }

    let mut asset_creation = assets.iter().fold(quote!(), |token_stream, asset| {
        asset.attach_token_stream_for_creation(token_stream)
    });
    asset_creation.append_all(default_fields.iter().fold(
        quote!(),
        |es, ident| quote! (#es #ident : Default::default()),
    ));
    let create_function = quote! {
            fn create(world: &mut World) -> Self {
                let cell = world.cell();
                let asset_server = cell.get_resource::<AssetServer>().expect("Cannot get AssetServer");
                let asset_keys = cell.get_resource::<bevy_asset_loader::AssetKeys>().expect("Cannot get bevy_asset_loader::AssetKeys");
                #conditional_asset_collections
                #name {
                    #asset_creation
                }
            }
    };

    let impl_asset_collection = quote! {
        #[automatically_derived]
        #[allow(unused_variables)]
        impl AssetCollection for #name {
            #create_function

            #load_function
        }
    };
    Ok(impl_asset_collection)
}

#[derive(Debug)]
enum ParseFieldError {
    NoAttributes,
    KeyAttributeStandsAlone,
    OnlyDynamicCanBeOptional,
    WrongAttributeType(proc_macro2::TokenStream, &'static str),
    UnknownAttributeType(proc_macro2::TokenStream),
    UnknownAttribute(proc_macro2::TokenStream),
    MissingAttributes(Vec<String>),
    #[allow(dead_code)]
    MissingRenderFeature(proc_macro2::TokenStream),
}

fn parse_field(field: &Field) -> Result<AssetField, Vec<ParseFieldError>> {
    let mut builder = AssetBuilder::default();
    let mut errors = vec![];
    for attr in field.attrs.iter() {
        if let syn::Meta::List(ref asset_meta_list) = attr.parse_meta().unwrap() {
            if *asset_meta_list.path.get_ident().unwrap() != ASSET_ATTRIBUTE {
                continue;
            }

            for attribute in asset_meta_list.nested.iter() {
                if let NestedMeta::Meta(Meta::NameValue(ref named_value)) = attribute {
                    let path = named_value.path.get_ident().unwrap().clone();
                    builder.field_ident = Some(field.clone().ident.unwrap());

                    if path == PATH_ATTRIBUTE {
                        if let Lit::Str(path_literal) = &named_value.lit {
                            builder.asset_path = Some(path_literal.value());
                        } else {
                            errors.push(ParseFieldError::WrongAttributeType(
                                named_value.into_token_stream(),
                                "str",
                            ));
                        }
                    } else if path == KEY_ATTRIBUTE {
                        if let Lit::Str(path_literal) = &named_value.lit {
                            builder.key = Some(path_literal.value());
                        } else {
                            errors.push(ParseFieldError::WrongAttributeType(
                                named_value.into_token_stream(),
                                "str",
                            ));
                        }
                    } else {
                        errors.push(ParseFieldError::UnknownAttribute(
                            named_value.into_token_stream(),
                        ))
                    }
                } else if let NestedMeta::Meta(Meta::Path(ref meta_path)) = attribute {
                    let path = meta_path.get_ident().unwrap().clone();
                    if path == STANDARD_MATERIAL_ATTRIBUTE {
                        #[cfg(not(feature = "render"))]
                        errors.push(ParseFieldError::MissingRenderFeature(
                            meta_path.into_token_stream(),
                        ));
                        #[cfg(feature = "render")]
                        {
                            builder.is_standard_material = true;
                        }
                    } else if path == OPTIONAL_ATTRIBUTE {
                        builder.is_optional = true;
                    } else if path == FOLDER_ATTRIBUTE {
                        builder.is_folder = true;
                    } else {
                        errors.push(ParseFieldError::UnknownAttribute(
                            meta_path.into_token_stream(),
                        ))
                    }
                } else if let NestedMeta::Meta(Meta::List(ref meta_list)) = attribute {
                    let path = meta_list.path.get_ident().unwrap().clone();
                    if path == TEXTURE_ATLAS_ATTRIBUTE {
                        #[cfg(not(feature = "render"))]
                        errors.push(ParseFieldError::MissingRenderFeature(
                            meta_list.into_token_stream(),
                        ));
                        #[cfg(feature = "render")]
                        for attribute in meta_list.nested.iter() {
                            if let NestedMeta::Meta(Meta::NameValue(ref named_value)) = attribute {
                                let path = named_value.path.get_ident().unwrap().clone();
                                if path == TextureAtlasAttribute::TILE_SIZE_X {
                                    if let Lit::Float(width) = &named_value.lit {
                                        builder.tile_size_x =
                                            Some(width.base10_parse::<f32>().unwrap());
                                    } else {
                                        errors.push(ParseFieldError::WrongAttributeType(
                                            named_value.into_token_stream(),
                                            "float",
                                        ));
                                    }
                                } else if path == TextureAtlasAttribute::TILE_SIZE_Y {
                                    if let Lit::Float(height) = &named_value.lit {
                                        builder.tile_size_y =
                                            Some(height.base10_parse::<f32>().unwrap());
                                    } else {
                                        errors.push(ParseFieldError::WrongAttributeType(
                                            named_value.into_token_stream(),
                                            "float",
                                        ));
                                    }
                                } else if path == TextureAtlasAttribute::COLUMNS {
                                    if let Lit::Int(columns) = &named_value.lit {
                                        builder.columns =
                                            Some(columns.base10_parse::<usize>().unwrap());
                                    } else {
                                        errors.push(ParseFieldError::WrongAttributeType(
                                            named_value.into_token_stream(),
                                            "integer",
                                        ));
                                    }
                                } else if path == TextureAtlasAttribute::ROWS {
                                    if let Lit::Int(rows) = &named_value.lit {
                                        builder.rows = Some(rows.base10_parse::<usize>().unwrap());
                                    } else {
                                        errors.push(ParseFieldError::WrongAttributeType(
                                            named_value.into_token_stream(),
                                            "integer",
                                        ));
                                    }
                                } else if path == TextureAtlasAttribute::PADDING_X {
                                    if let Lit::Float(padding_x) = &named_value.lit {
                                        builder.padding_x =
                                            Some(padding_x.base10_parse::<f32>().unwrap());
                                    } else {
                                        errors.push(ParseFieldError::WrongAttributeType(
                                            named_value.into_token_stream(),
                                            "float",
                                        ));
                                    }
                                } else if path == TextureAtlasAttribute::PADDING_Y {
                                    if let Lit::Float(padding_y) = &named_value.lit {
                                        builder.padding_y =
                                            Some(padding_y.base10_parse::<f32>().unwrap());
                                    } else {
                                        errors.push(ParseFieldError::WrongAttributeType(
                                            named_value.into_token_stream(),
                                            "float",
                                        ));
                                    }
                                } else {
                                    errors.push(ParseFieldError::UnknownAttribute(
                                        named_value.into_token_stream(),
                                    ));
                                }
                            } else {
                                errors.push(ParseFieldError::UnknownAttributeType(
                                    attribute.into_token_stream(),
                                ));
                            }
                        }
                    } else if path == FOLDER_ATTRIBUTE {
                        for attribute in meta_list.nested.iter() {
                            if let NestedMeta::Meta(Meta::Path(ref meta_path)) = attribute {
                                let path = meta_path.get_ident().unwrap().clone();
                                if path == FOLDER_TYPED_ATTRIBUTE {
                                    builder.is_folder = true;
                                    builder.is_typed_folder = true;
                                } else {
                                    errors.push(ParseFieldError::UnknownAttribute(
                                        meta_path.into_token_stream(),
                                    ))
                                }
                            } else {
                                errors.push(ParseFieldError::UnknownAttributeType(
                                    attribute.into_token_stream(),
                                ));
                            }
                        }
                    } else {
                        errors.push(ParseFieldError::UnknownAttribute(
                            meta_list.into_token_stream(),
                        ))
                    }
                } else {
                    errors.push(ParseFieldError::UnknownAttributeType(
                        attribute.into_token_stream(),
                    ));
                }
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    builder.build()
}

fn to_compile_errors(errors: Vec<syn::Error>) -> proc_macro2::TokenStream {
    let compile_errors = errors.iter().map(syn::Error::to_compile_error);
    quote!(#(#compile_errors)*)
}
