use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::mem::discriminant;

use graph::prelude::*;
use graph::{components::store::EntityType, data::graphql::ObjectOrInterface};

use crate::schema::ast as sast;
use crate::store::prefetch::ObjectCondition;

#[derive(Debug)]
enum OrderDirection {
    Ascending,
    Descending,
}

/// Builds a EntityQuery from GraphQL arguments.
///
/// Panics if `entity` is not present in `schema`.
pub fn build_query<'a>(
    entity: impl Into<ObjectOrInterface<'a>>,
    block: BlockNumber,
    arguments: &HashMap<&str, r::Value>,
    types_for_interface: &'a BTreeMap<EntityType, Vec<s::ObjectType>>,
    max_first: u32,
    max_skip: u32,
    mut column_names: BTreeMap<ObjectCondition<'a>, AttributeNames>,
) -> Result<EntityQuery, QueryExecutionError> {
    let entity = entity.into();
    let entity_types = EntityCollection::All(match &entity {
        ObjectOrInterface::Object(object) => {
            let selected_columns = column_names
                .remove(&(*object).into())
                .unwrap_or(AttributeNames::All);
            vec![((*object).into(), selected_columns)]
        }
        ObjectOrInterface::Interface(interface) => types_for_interface
            [&EntityType::from(*interface)]
            .iter()
            .map(|o| {
                let selected_columns = column_names
                    .remove(&o.into())
                    .unwrap_or(AttributeNames::All);
                (o.into(), selected_columns)
            })
            .collect(),
    });
    let mut query = EntityQuery::new(parse_subgraph_id(entity)?, block, entity_types)
        .range(build_range(arguments, max_first, max_skip)?);
    if let Some(filter) = build_filter(entity, arguments)? {
        query = query.filter(filter);
    }
    let order = match (
        build_order_by(entity, arguments)?,
        build_order_direction(arguments)?,
    ) {
        (Some((attr, value_type)), OrderDirection::Ascending) => {
            EntityOrder::Ascending(attr, value_type)
        }
        (Some((attr, value_type)), OrderDirection::Descending) => {
            EntityOrder::Descending(attr, value_type)
        }
        (None, _) => EntityOrder::Default,
    };
    query = query.order(order);
    Ok(query)
}

/// Parses GraphQL arguments into a EntityRange, if present.
fn build_range(
    arguments: &HashMap<&str, r::Value>,
    max_first: u32,
    max_skip: u32,
) -> Result<EntityRange, QueryExecutionError> {
    let first = match arguments.get("first") {
        Some(r::Value::Int(n)) => {
            let n = *n;
            if n > 0 && n <= (max_first as i64) {
                n as u32
            } else {
                return Err(QueryExecutionError::RangeArgumentsError(
                    "first", max_first, n,
                ));
            }
        }
        Some(r::Value::Null) | None => 100,
        _ => unreachable!("first is an Int with a default value"),
    };

    let skip = match arguments.get("skip") {
        Some(r::Value::Int(n)) => {
            let n = *n;
            if n >= 0 && n <= (max_skip as i64) {
                n as u32
            } else {
                return Err(QueryExecutionError::RangeArgumentsError(
                    "skip", max_skip, n,
                ));
            }
        }
        Some(r::Value::Null) | None => 0,
        _ => unreachable!("skip is an Int with a default value"),
    };

    Ok(EntityRange {
        first: Some(first),
        skip,
    })
}

/// Parses GraphQL arguments into an EntityFilter, if present.
fn build_filter(
    entity: ObjectOrInterface,
    arguments: &HashMap<&str, r::Value>,
) -> Result<Option<EntityFilter>, QueryExecutionError> {
    match arguments.get("where") {
        Some(r::Value::Object(object)) => build_filter_from_object(entity, object),
        Some(r::Value::Null) => Ok(None),
        None => match arguments.get("text") {
            Some(r::Value::Object(filter)) => build_fulltext_filter_from_object(filter),
            None => Ok(None),
            _ => Err(QueryExecutionError::InvalidFilterError),
        },
        _ => Err(QueryExecutionError::InvalidFilterError),
    }
}

fn build_fulltext_filter_from_object(
    object: &BTreeMap<String, r::Value>,
) -> Result<Option<EntityFilter>, QueryExecutionError> {
    object.iter().next().map_or(
        Err(QueryExecutionError::FulltextQueryRequiresFilter),
        |(key, value)| {
            if let r::Value::String(s) = value {
                Ok(Some(EntityFilter::Equal(
                    key.clone(),
                    Value::String(s.clone()),
                )))
            } else {
                Err(QueryExecutionError::FulltextQueryRequiresFilter)
            }
        },
    )
}

/// Parses a GraphQL input object into an EntityFilter, if present.
fn build_filter_from_object(
    entity: ObjectOrInterface,
    object: &BTreeMap<String, r::Value>,
) -> Result<Option<EntityFilter>, QueryExecutionError> {
    Ok(Some(EntityFilter::And({
        object
            .iter()
            .map(|(key, value)| {
                use self::sast::FilterOp::*;

                let (field_name, op) = sast::parse_field_as_filter(key);

                let field = sast::get_field(entity, &field_name).ok_or_else(|| {
                    QueryExecutionError::EntityFieldError(
                        entity.name().to_owned(),
                        field_name.clone(),
                    )
                })?;

                let ty = &field.field_type;
                let store_value = Value::from_query_value(value, ty)?;

                Ok(match op {
                    Not => EntityFilter::Not(field_name, store_value),
                    GreaterThan => EntityFilter::GreaterThan(field_name, store_value),
                    LessThan => EntityFilter::LessThan(field_name, store_value),
                    GreaterOrEqual => EntityFilter::GreaterOrEqual(field_name, store_value),
                    LessOrEqual => EntityFilter::LessOrEqual(field_name, store_value),
                    In => EntityFilter::In(field_name, list_values(store_value, "_in")?),
                    NotIn => EntityFilter::NotIn(field_name, list_values(store_value, "_not_in")?),
                    Contains => EntityFilter::Contains(field_name, store_value),
                    NotContains => EntityFilter::NotContains(field_name, store_value),
                    StartsWith => EntityFilter::StartsWith(field_name, store_value),
                    NotStartsWith => EntityFilter::NotStartsWith(field_name, store_value),
                    EndsWith => EntityFilter::EndsWith(field_name, store_value),
                    NotEndsWith => EntityFilter::NotEndsWith(field_name, store_value),
                    Equal => EntityFilter::Equal(field_name, store_value),
                })
            })
            .collect::<Result<Vec<EntityFilter>, QueryExecutionError>>()?
    })))
}

/// Parses a list of GraphQL values into a vector of entity field values.
fn list_values(value: Value, filter_type: &str) -> Result<Vec<Value>, QueryExecutionError> {
    match value {
        Value::List(ref values) if !values.is_empty() => {
            // Check that all values in list are of the same type
            let root_discriminant = discriminant(&values[0]);
            values
                .iter()
                .map(|value| {
                    let current_discriminant = discriminant(value);
                    if root_discriminant == current_discriminant {
                        Ok(value.clone())
                    } else {
                        Err(QueryExecutionError::ListTypesError(
                            filter_type.to_string(),
                            vec![values[0].to_string(), value.to_string()],
                        ))
                    }
                })
                .collect::<Result<Vec<_>, _>>()
        }
        Value::List(ref values) if values.is_empty() => Ok(vec![]),
        _ => Err(QueryExecutionError::ListFilterError(
            filter_type.to_string(),
        )),
    }
}

/// Parses GraphQL arguments into an field name to order by, if present.
fn build_order_by(
    entity: ObjectOrInterface,
    arguments: &HashMap<&str, r::Value>,
) -> Result<Option<(String, ValueType)>, QueryExecutionError> {
    match arguments.get("orderBy") {
        Some(r::Value::Enum(name)) => {
            let field = sast::get_field(entity, name).ok_or_else(|| {
                QueryExecutionError::EntityFieldError(entity.name().to_owned(), name.clone())
            })?;
            sast::get_field_value_type(&field.field_type)
                .map(|value_type| Some((name.to_owned(), value_type)))
                .map_err(|_| {
                    QueryExecutionError::OrderByNotSupportedError(
                        entity.name().to_owned(),
                        name.clone(),
                    )
                })
        }
        _ => match arguments.get("text") {
            Some(r::Value::Object(filter)) => build_fulltext_order_by_from_object(filter),
            None => Ok(None),
            _ => Err(QueryExecutionError::InvalidFilterError),
        },
    }
}

fn build_fulltext_order_by_from_object(
    object: &BTreeMap<String, r::Value>,
) -> Result<Option<(String, ValueType)>, QueryExecutionError> {
    object.iter().next().map_or(
        Err(QueryExecutionError::FulltextQueryRequiresFilter),
        |(key, value)| {
            if let r::Value::String(_) = value {
                Ok(Some((key.clone(), ValueType::String)))
            } else {
                Err(QueryExecutionError::FulltextQueryRequiresFilter)
            }
        },
    )
}

/// Parses GraphQL arguments into a EntityOrder, if present.
fn build_order_direction(
    arguments: &HashMap<&str, r::Value>,
) -> Result<OrderDirection, QueryExecutionError> {
    Ok(arguments
        .get("orderDirection")
        .map(|value| match value {
            r::Value::Enum(name) if name == "asc" => OrderDirection::Ascending,
            r::Value::Enum(name) if name == "desc" => OrderDirection::Descending,
            _ => OrderDirection::Ascending,
        })
        .unwrap_or(OrderDirection::Ascending))
}

/// Parses the subgraph ID from the ObjectType directives.
pub fn parse_subgraph_id<'a>(
    entity: impl Into<ObjectOrInterface<'a>>,
) -> Result<DeploymentHash, QueryExecutionError> {
    let entity = entity.into();
    let entity_name = entity.name();
    entity
        .directives()
        .iter()
        .find(|directive| directive.name == "subgraphId")
        .and_then(|directive| directive.arguments.iter().find(|(name, _)| name == "id"))
        .and_then(|(_, value)| match value {
            s::Value::String(id) => Some(id),
            _ => None,
        })
        .ok_or(())
        .and_then(|id| DeploymentHash::new(id).map_err(|_| ()))
        .map_err(|_| QueryExecutionError::SubgraphDeploymentIdError(entity_name.to_owned()))
}

/// Recursively collects entities involved in a query field as `(subgraph ID, name)` tuples.
pub fn collect_entities_from_query_field(
    schema: &s::Document,
    object_type: &s::ObjectType,
    field: &q::Field,
) -> BTreeSet<SubscriptionFilter> {
    // Output entities
    let mut entities = HashSet::new();

    // List of objects/fields to visit next
    let mut queue = VecDeque::new();
    queue.push_back((object_type, field));

    while let Some((object_type, field)) = queue.pop_front() {
        // Check if the field exists on the object type
        if let Some(field_type) = sast::get_field(object_type, &field.name) {
            // Check if the field type corresponds to a type definition (in a valid schema,
            // this should always be the case)
            if let Some(type_definition) = sast::get_type_definition_from_field(schema, field_type)
            {
                // If the field's type definition is an object type, extract that type
                if let s::TypeDefinition::Object(object_type) = type_definition {
                    // Only collect whether the field's type has an @entity directive
                    if sast::get_object_type_directive(object_type, String::from("entity"))
                        .is_some()
                    {
                        // Obtain the subgraph ID from the object type
                        if let Ok(subgraph_id) = parse_subgraph_id(object_type) {
                            // Add the (subgraph_id, entity_name) tuple to the result set
                            entities.insert((subgraph_id, object_type.name.to_owned()));
                        }
                    }

                    // If the query field has a non-empty selection set, this means we
                    // need to recursively process it
                    for selection in field.selection_set.items.iter() {
                        if let q::Selection::Field(sub_field) = selection {
                            queue.push_back((object_type, sub_field))
                        }
                    }
                }
            }
        }
    }

    entities
        .into_iter()
        .map(|(id, entity_type)| SubscriptionFilter::Entities(id, EntityType::new(entity_type)))
        .collect()
}

#[cfg(test)]
mod tests {
    use graph::{
        components::store::EntityType,
        prelude::s::{Directive, Field, InputValue, ObjectType, Type, Value as SchemaValue},
    };
    use graphql_parser::Pos;
    use std::collections::{BTreeMap, HashMap};

    use graph::prelude::*;

    use super::build_query;

    fn default_object() -> ObjectType {
        let subgraph_id_argument = (
            String::from("id"),
            s::Value::String("QmZ5dsusHwD1PEbx6L4dLCWkDsk1BLhrx9mPsGyPvTxPCM".to_string()),
        );
        let subgraph_id_directive = Directive {
            name: "subgraphId".to_string(),
            position: Pos::default(),
            arguments: vec![subgraph_id_argument],
        };
        let name_input_value = InputValue {
            position: Pos::default(),
            description: Some("name input".to_string()),
            name: "name".to_string(),
            value_type: Type::NamedType("String".to_string()),
            default_value: Some(SchemaValue::String("name".to_string())),
            directives: vec![],
        };
        let name_field = Field {
            position: Pos::default(),
            description: Some("name field".to_string()),
            name: "name".to_string(),
            arguments: vec![name_input_value.clone()],
            field_type: Type::NamedType("String".to_string()),
            directives: vec![],
        };
        let email_field = Field {
            position: Pos::default(),
            description: Some("email field".to_string()),
            name: "email".to_string(),
            arguments: vec![name_input_value],
            field_type: Type::NamedType("String".to_string()),
            directives: vec![],
        };

        ObjectType {
            position: Default::default(),
            description: None,
            name: String::new(),
            implements_interfaces: vec![],
            directives: vec![subgraph_id_directive],
            fields: vec![name_field, email_field],
        }
    }

    fn object(name: &str) -> ObjectType {
        ObjectType {
            name: name.to_owned(),
            ..default_object()
        }
    }

    fn field(name: &str, field_type: Type) -> Field {
        Field {
            position: Default::default(),
            description: None,
            name: name.to_owned(),
            arguments: vec![],
            field_type,
            directives: vec![],
        }
    }

    fn default_arguments<'a>() -> HashMap<&'a str, r::Value> {
        let mut map = HashMap::new();
        let first = "first";
        let skip = "skip";
        map.insert(first, r::Value::Int(100.into()));
        map.insert(skip, r::Value::Int(0.into()));
        map
    }

    #[test]
    fn build_query_uses_the_entity_name() {
        assert_eq!(
            build_query(
                &object("Entity1"),
                BLOCK_NUMBER_MAX,
                &default_arguments(),
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .collection,
            EntityCollection::All(vec![(EntityType::from("Entity1"), AttributeNames::All)])
        );
        assert_eq!(
            build_query(
                &object("Entity2"),
                BLOCK_NUMBER_MAX,
                &default_arguments(),
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .collection,
            EntityCollection::All(vec![(EntityType::from("Entity2"), AttributeNames::All)])
        );
    }

    #[test]
    fn build_query_yields_no_order_if_order_arguments_are_missing() {
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &default_arguments(),
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Default,
        );
    }

    #[test]
    fn build_query_parses_order_by_from_enum_values_correctly() {
        let order_by = "orderBy".to_string();
        let mut args = default_arguments();
        args.insert(&order_by, r::Value::Enum("name".to_string()));
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Ascending("name".to_string(), ValueType::String)
        );

        let mut args = default_arguments();
        args.insert(&order_by, r::Value::Enum("email".to_string()));
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Ascending("email".to_string(), ValueType::String)
        );
    }

    #[test]
    fn build_query_ignores_order_by_from_non_enum_values() {
        let order_by = "orderBy".to_string();
        let mut args = default_arguments();
        args.insert(&order_by, r::Value::String("name".to_string()));
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Default
        );

        let mut args = default_arguments();
        args.insert(&order_by, r::Value::String("email".to_string()));
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Default
        );
    }

    #[test]
    fn build_query_parses_order_direction_from_enum_values_correctly() {
        let order_by = "orderBy".to_string();
        let order_direction = "orderDirection".to_string();
        let mut args = default_arguments();
        args.insert(&order_by, r::Value::Enum("name".to_string()));
        args.insert(&order_direction, r::Value::Enum("asc".to_string()));
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Ascending("name".to_string(), ValueType::String)
        );

        let mut args = default_arguments();
        args.insert(&order_by, r::Value::Enum("name".to_string()));
        args.insert(&order_direction, r::Value::Enum("desc".to_string()));
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Descending("name".to_string(), ValueType::String)
        );

        let mut args = default_arguments();
        args.insert(&order_by, r::Value::Enum("name".to_string()));
        args.insert(
            &order_direction,
            r::Value::Enum("descending...".to_string()),
        );
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Ascending("name".to_string(), ValueType::String)
        );

        // No orderBy -> EntityOrder::Default
        let mut args = default_arguments();
        args.insert(
            &order_direction,
            r::Value::Enum("descending...".to_string()),
        );
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Default
        );
    }

    #[test]
    fn build_query_ignores_order_direction_from_non_enum_values() {
        let order_by = "orderBy".to_string();
        let order_direction = "orderDirection".to_string();
        let mut args = default_arguments();
        args.insert(&order_by, r::Value::Enum("name".to_string()));
        args.insert(&order_direction, r::Value::String("asc".to_string()));
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Ascending("name".to_string(), ValueType::String)
        );

        let mut args = default_arguments();
        args.insert(&order_by, r::Value::Enum("name".to_string()));
        args.insert(&order_direction, r::Value::String("desc".to_string()));
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .order,
            EntityOrder::Ascending("name".to_string(), ValueType::String)
        );
    }

    #[test]
    fn build_query_yields_default_range_if_none_is_present() {
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &default_arguments(),
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .range,
            EntityRange::first(100)
        );
    }

    #[test]
    fn build_query_yields_default_first_if_only_skip_is_present() {
        let skip = "skip".to_string();
        let mut args = default_arguments();
        args.insert(&skip, r::Value::Int(50));
        assert_eq!(
            build_query(
                &default_object(),
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .range,
            EntityRange {
                first: Some(100),
                skip: 50,
            },
        );
    }

    #[test]
    fn build_query_yields_filters() {
        let whre = "where".to_string();
        let mut args = default_arguments();
        args.insert(
            &whre,
            r::Value::Object(BTreeMap::from_iter(vec![(
                "name_ends_with".to_string(),
                r::Value::String("ello".to_string()),
            )])),
        );
        assert_eq!(
            build_query(
                &ObjectType {
                    fields: vec![field("name", Type::NamedType("string".to_owned()))],
                    ..default_object()
                },
                BLOCK_NUMBER_MAX,
                &args,
                &BTreeMap::new(),
                std::u32::MAX,
                std::u32::MAX,
                Default::default()
            )
            .unwrap()
            .filter,
            Some(EntityFilter::And(vec![EntityFilter::EndsWith(
                "name".to_string(),
                Value::String("ello".to_string()),
            )]))
        )
    }
}
