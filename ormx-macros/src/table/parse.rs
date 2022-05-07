use std::{convert::TryFrom, marker::PhantomData};

use proc_macro2::Span;
use syn::{Data, DeriveInput, Error, Ident, Result};

use super::{DefaultType, Table, TableField};
use crate::{
    attrs::{parse_attrs, Insertable, TableAttr, TableFieldAttr},
    backend::Backend,
    utils::{missing_attr, set_once},
};

macro_rules! none {
    ($($i:ident),*) => { $( let mut $i = None; )* };
}

impl<B: Backend> TryFrom<&syn::Field> for TableField<B> {
    type Error = Error;

    fn try_from(value: &syn::Field) -> Result<Self> {
        let ident = value.ident.clone().unwrap();

        let reserved_ident = B::RESERVED_IDENTS.contains(&&*ident.to_string().to_uppercase());
        if reserved_ident {
            proc_macro_error::emit_warning!(
                ident.span(),
                "This is a reserved keyword, you might want to consider choosing a different name."
            );
        }

        none!(
            column,
            custom_type,
            get_one,
            get_optional,
            get_many,
            set,
            default,
            by_ref,
            set_as_wildcard
        );
        let mut insert_attrs = vec![];

        for attr in parse_attrs::<TableFieldAttr>(&value.attrs)? {
            match attr {
                TableFieldAttr::Column(c) => set_once(&mut column, c)?,
                TableFieldAttr::CustomType(..) => set_once(&mut custom_type, true)?,
                TableFieldAttr::GetOne(g) => set_once(&mut get_one, g)?,
                TableFieldAttr::GetOptional(g) => set_once(&mut get_optional, g)?,
                TableFieldAttr::GetMany(g) => set_once(&mut get_many, g)?,
                TableFieldAttr::Set(s) => {
                    let default = || Ident::new(&format!("set_{}", ident), Span::call_site());
                    set_once(&mut set, s.unwrap_or_else(default))?
                }
                TableFieldAttr::Default(literal) => set_once(
                    &mut default,
                    match literal {
                        Some(literal) => match literal.to_string().as_str() {
                            "\"always\"" => DefaultType::Always,
                            "\"insert\"" => DefaultType::Insert,
                            _ => proc_macro_error::abort!(
                                literal,
                                "allowed values: \"always\", \"insert\""
                            ),
                        },
                        None => DefaultType::Insert,
                    },
                )?,
                TableFieldAttr::ByRef(..) => set_once(&mut by_ref, true)?,
                TableFieldAttr::SetAsWildcard(..) => set_once(&mut set_as_wildcard, true)?,
                TableFieldAttr::InsertAttr(mut attr) => insert_attrs.append(&mut attr.0),
            }
        }
        Ok(TableField {
            column_name: column.unwrap_or_else(|| ident.to_string()),
            field: ident,
            ty: value.ty.clone(),
            custom_type: custom_type.unwrap_or(false),
            reserved_ident,
            default,
            get_one,
            get_optional,
            get_many,
            set,
            by_ref: by_ref.unwrap_or(false),
            set_as_wildcard: set_as_wildcard.unwrap_or(false),
            insert_attrs,
            _phantom: PhantomData,
        })
    }
}

impl<B: Backend> TryFrom<&syn::DeriveInput> for Table<B> {
    type Error = Error;

    fn try_from(value: &DeriveInput) -> Result<Self> {
        let data = match &value.data {
            Data::Struct(s) => s,
            _ => panic!("not a struct with named fields"),
        };

        let mut fields = data
            .fields
            .iter()
            .map(TableField::try_from)
            .collect::<Result<Vec<_>>>()?;

        none!(table, id, insertable, deletable);
        for attr in parse_attrs::<TableAttr>(&value.attrs)? {
            match attr {
                TableAttr::Table(x) => set_once(&mut table, x)?,
                TableAttr::Id(x) => set_once(&mut id, x)?,
                TableAttr::Insertable(x) => {
                    let default = || Insertable {
                        attrs: vec![],
                        ident: Ident::new(&format!("Insert{}", value.ident), Span::call_site()),
                    };
                    set_once(&mut insertable, x.unwrap_or_else(default))?;
                }
                TableAttr::Deletable(_) => set_once(&mut deletable, true)?,
            }
        }

        let id = id.unwrap_or_else(|| Ident::new("id", Span::call_site()));
        let id = match fields.iter_mut().find(|field| field.field == id) {
            Some(id) => id,
            None => proc_macro_error::abort!(id, "id field does not exist in struct"),
        };
        id.default = match id.default {
            None => Some(DefaultType::Always),
            Some(_) => proc_macro_error::abort!(id.field, "the id field is always always-default and another default-behavior cannot be specified"),
        };
        let id = id.clone();

        for field in fields.iter() {
            if field.set.is_some() && field.default == Some(DefaultType::Always) {
                proc_macro_error::abort!(
                    field.field,
                    "an always-default field cannot have a setter"
                );
            }
        }

        let table = table.ok_or_else(|| missing_attr("table"))?;
        let reserved_table_name = B::RESERVED_IDENTS.contains(&&*table.to_string().to_uppercase());
        if reserved_table_name {
            proc_macro_error::emit_warning!(
                Span::call_site(),
                "This table name is a reserved keyword, you might want to consider choosing a different name."
            );
        }

        Ok(Table {
            ident: value.ident.clone(),
            vis: value.vis.clone(),
            table,
            reserved_table_name,
            id,
            insertable,
            fields,
            deletable: deletable.unwrap_or(false),
        })
    }
}
