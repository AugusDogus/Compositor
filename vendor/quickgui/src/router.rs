use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use thiserror::Error;

/// Maximum routes retained by one router.
pub const MAX_ROUTES: usize = 1_024;
/// Maximum ancestors, including pathless layouts, in one matched route chain.
pub const MAX_ROUTE_DEPTH: usize = 64;
/// Maximum segments in one compiled route pattern or navigated pathname.
pub const MAX_ROUTE_SEGMENTS: usize = 128;
/// Maximum bytes in one application-declared route identifier.
pub const MAX_ROUTE_ID_BYTES: usize = 256;
/// Maximum bytes in one fully resolved route pattern.
pub const MAX_ROUTE_PATTERN_BYTES: usize = 2_048;
/// Maximum bytes in one resolved internal destination, including query and fragment.
pub const MAX_ROUTE_DESTINATION_BYTES: usize = 8_192;
/// Maximum decoded query pairs retained by one location.
pub const MAX_ROUTE_QUERY_PAIRS: usize = 256;
/// Maximum entries retained by one router's in-memory navigation history.
pub const MAX_ROUTE_HISTORY_ENTRIES: usize = 256;

/// One declarative route in a router-owned tree.
///
/// A route with a path participates in matching. A pathless route is a layout ancestor: it is
/// returned in the matched route chain but never wins a match by itself. Relative child paths are
/// joined to their parent's fully resolved pattern; an absolute child path starts at the root
/// while keeping the declared layout ancestry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteDefinition {
    id: Arc<str>,
    path: Option<Arc<str>>,
    parent_id: Option<Arc<str>>,
}

impl RouteDefinition {
    /// Declare a route that participates in path matching.
    pub fn new(id: impl Into<Arc<str>>, path: impl Into<Arc<str>>) -> Self {
        Self {
            id: id.into(),
            path: Some(path.into()),
            parent_id: None,
        }
    }

    /// Declare a pathless layout that wraps its matched descendants.
    pub fn layout(id: impl Into<Arc<str>>) -> Self {
        Self {
            id: id.into(),
            path: None,
            parent_id: None,
        }
    }

    /// Attach this route to a declared parent. Parents may appear before or after their children.
    #[must_use]
    pub fn parent(mut self, parent_id: impl Into<Arc<str>>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn parent_id(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }
}

/// One decoded dynamic parameter from the route pattern that won the current match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteParameter {
    name: Arc<str>,
    value: Arc<str>,
}

impl RouteParameter {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

/// One decoded query pair. Repeated names remain separate and preserve source order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteQueryPair {
    name: Arc<str>,
    value: Arc<str>,
}

impl RouteQueryPair {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

/// A normalized internal application location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteLocation {
    href: Arc<str>,
    pathname: Arc<str>,
    search: Arc<str>,
    hash: Arc<str>,
    query: Arc<[RouteQueryPair]>,
    decoded_segments: Arc<[Arc<str>]>,
}

impl RouteLocation {
    pub fn href(&self) -> &str {
        &self.href
    }

    pub fn pathname(&self) -> &str {
        &self.pathname
    }

    /// The raw query beginning with `?`, or the empty string.
    pub fn search(&self) -> &str {
        &self.search
    }

    /// The raw fragment beginning with `#`, or the empty string.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// Decoded query pairs in declaration order. Repeated names are not collapsed.
    pub fn query(&self) -> &[RouteQueryPair] {
        &self.query
    }
}

/// The route and layout ancestry selected for one location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteMatch {
    route_ids: Arc<[Arc<str>]>,
    params: Arc<[RouteParameter]>,
}

impl RouteMatch {
    /// Route IDs from the outermost layout through the winning leaf route.
    pub fn route_ids(&self) -> &[Arc<str>] {
        &self.route_ids
    }

    /// Decoded dynamic and wildcard parameters in pattern order.
    pub fn params(&self) -> &[RouteParameter] {
        &self.params
    }
}

/// Copy-on-read state exposed to renderer bindings after one navigation operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouterSnapshot {
    pub location: RouteLocation,
    pub matched: Option<RouteMatch>,
    pub history_index: usize,
    pub history_length: usize,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

/// Invalid route declarations or internal destinations.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RouterError {
    #[error("a router retains at most {limit} routes")]
    TooManyRoutes { limit: usize },
    #[error("route IDs must not be empty")]
    EmptyRouteId,
    #[error("route ID `{id}` exceeds {limit} bytes")]
    RouteIdTooLong { id: Arc<str>, limit: usize },
    #[error("duplicate route ID `{id}`")]
    DuplicateRouteId { id: Arc<str> },
    #[error("route `{route}` refers to missing parent `{parent}`")]
    MissingParent { route: Arc<str>, parent: Arc<str> },
    #[error("route ancestry contains a cycle at `{id}`")]
    ParentCycle { id: Arc<str> },
    #[error("route `{id}` exceeds the maximum ancestry depth of {limit}")]
    RouteTooDeep { id: Arc<str>, limit: usize },
    #[error("route pattern `{pattern}` exceeds {limit} bytes")]
    PatternTooLong { pattern: Arc<str>, limit: usize },
    #[error("route pattern `{pattern}` contains more than {limit} segments")]
    TooManyPatternSegments { pattern: Arc<str>, limit: usize },
    #[error("invalid route pattern `{pattern}`: {reason}")]
    InvalidPattern {
        pattern: Arc<str>,
        reason: &'static str,
    },
    #[error("destination exceeds {limit} bytes")]
    DestinationTooLong { limit: usize },
    #[error("invalid internal destination `{destination}`: {reason}")]
    InvalidDestination {
        destination: Arc<str>,
        reason: &'static str,
    },
    #[error("location contains more than {limit} query pairs")]
    TooManyQueryPairs { limit: usize },
    #[error("invalid percent encoding in `{component}`")]
    InvalidPercentEncoding { component: Arc<str> },
    #[error("percent-decoded URL component is not UTF-8")]
    InvalidPercentEncodedUtf8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PatternSegment {
    Static(Arc<str>),
    Parameter(Arc<str>),
    OptionalParameter(Arc<str>),
    Wildcard(Arc<str>),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RouteSpecificity {
    static_segments: usize,
    required_segments: usize,
    optional_segments: usize,
    wildcard: bool,
    ancestry_depth: usize,
}

impl RouteSpecificity {
    fn compare(self, other: Self) -> Ordering {
        self.static_segments
            .cmp(&other.static_segments)
            .then_with(|| self.required_segments.cmp(&other.required_segments))
            .then_with(|| other.wildcard.cmp(&self.wildcard))
            .then_with(|| other.optional_segments.cmp(&self.optional_segments))
            .then_with(|| self.ancestry_depth.cmp(&other.ancestry_depth))
    }
}

#[derive(Clone, Debug)]
struct CompiledRoute {
    route_ids: Arc<[Arc<str>]>,
    segments: Arc<[PatternSegment]>,
    specificity: RouteSpecificity,
}

#[derive(Clone, Debug)]
struct ResolvedDefinition {
    full_pattern: Arc<str>,
    route_ids: Arc<[Arc<str>]>,
    participates_in_matching: bool,
}

/// Core-owned, bounded route matching and in-memory navigation history.
///
/// The router retains no window, task, timer, observer, or scheduler source. Mutations happen only
/// when the application pushes, replaces, or traverses history, so renderer bindings can call it
/// synchronously without waiting for native main-thread work.
#[derive(Clone, Debug)]
pub struct Router {
    routes: Vec<CompiledRoute>,
    history: Vec<RouteLocation>,
    history_index: usize,
    matched: Option<RouteMatch>,
}

impl Router {
    /// Compile a declarative route tree and begin its memory history at `initial_destination`.
    pub fn new(
        definitions: impl IntoIterator<Item = RouteDefinition>,
        initial_destination: impl AsRef<str>,
    ) -> Result<Self, RouterError> {
        let definitions: Vec<RouteDefinition> = definitions.into_iter().collect();
        if definitions.len() > MAX_ROUTES {
            return Err(RouterError::TooManyRoutes { limit: MAX_ROUTES });
        }

        let mut indices = HashMap::with_capacity(definitions.len());
        for (index, definition) in definitions.iter().enumerate() {
            validate_route_id(&definition.id)?;
            if indices.insert(definition.id.clone(), index).is_some() {
                return Err(RouterError::DuplicateRouteId {
                    id: definition.id.clone(),
                });
            }
        }

        let mut marks = vec![0_u8; definitions.len()];
        let mut resolved = vec![None; definitions.len()];
        for index in 0..definitions.len() {
            resolve_definition(index, &definitions, &indices, &mut marks, &mut resolved)?;
        }

        let mut routes = Vec::with_capacity(definitions.len());
        for definition in resolved.into_iter().flatten() {
            if !definition.participates_in_matching {
                continue;
            }
            let (segments, mut specificity) = compile_pattern(&definition.full_pattern)?;
            specificity.ancestry_depth = definition.route_ids.len();
            routes.push(CompiledRoute {
                route_ids: definition.route_ids,
                segments: segments.into(),
                specificity,
            });
        }

        let root = root_location();
        let location = resolve_location(&root, initial_destination.as_ref())?;
        let matched = match_location(&routes, &location);
        Ok(Self {
            routes,
            history: vec![location],
            history_index: 0,
            matched,
        })
    }

    pub fn location(&self) -> &RouteLocation {
        &self.history[self.history_index]
    }

    pub fn matched(&self) -> Option<&RouteMatch> {
        self.matched.as_ref()
    }

    pub fn history_index(&self) -> usize {
        self.history_index
    }

    pub fn history_length(&self) -> usize {
        self.history.len()
    }

    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    pub fn snapshot(&self) -> RouterSnapshot {
        RouterSnapshot {
            location: self.location().clone(),
            matched: self.matched.clone(),
            history_index: self.history_index,
            history_length: self.history.len(),
            can_go_back: self.can_go_back(),
            can_go_forward: self.can_go_forward(),
        }
    }

    /// Resolve an absolute, relative, query-only, or fragment-only destination without mutating.
    pub fn resolve(&self, destination: &str) -> Result<RouteLocation, RouterError> {
        resolve_location(self.location(), destination)
    }

    /// Append one location, discard forward entries, and select the new route match.
    pub fn push(&mut self, destination: &str) -> Result<(), RouterError> {
        let location = self.resolve(destination)?;
        self.history.truncate(self.history_index + 1);
        if self.history.len() == MAX_ROUTE_HISTORY_ENTRIES {
            self.history.remove(0);
        }
        self.history.push(location);
        self.history_index = self.history.len() - 1;
        self.rematch();
        Ok(())
    }

    /// Replace the current location without changing history length.
    ///
    /// Returns whether the normalized location changed.
    pub fn replace(&mut self, destination: &str) -> Result<bool, RouterError> {
        let location = self.resolve(destination)?;
        if self.location() == &location {
            return Ok(false);
        }
        self.history[self.history_index] = location;
        self.rematch();
        Ok(true)
    }

    /// Traverse memory history, clamping an oversized delta to the first or last entry.
    pub fn go(&mut self, delta: isize) -> bool {
        if delta == 0 || self.history.is_empty() {
            return false;
        }
        let next = self
            .history_index
            .saturating_add_signed(delta)
            .min(self.history.len() - 1);
        if next == self.history_index {
            return false;
        }
        self.history_index = next;
        self.rematch();
        true
    }

    pub fn back(&mut self) -> bool {
        self.go(-1)
    }

    pub fn forward(&mut self) -> bool {
        self.go(1)
    }

    /// Whether `destination` resolves to the current pathname.
    ///
    /// With `end == false`, descendants also count as active on a segment boundary. Query and
    /// fragment differences do not affect active state.
    pub fn is_active(&self, destination: &str, end: bool) -> Result<bool, RouterError> {
        let target = self.resolve(destination)?;
        let current = self.location().pathname();
        let target = target.pathname();
        if current == target {
            return Ok(true);
        }
        if end {
            return Ok(false);
        }
        if target == "/" {
            return Ok(current.starts_with('/'));
        }
        Ok(current
            .strip_prefix(target)
            .is_some_and(|suffix| suffix.starts_with('/')))
    }

    fn rematch(&mut self) {
        self.matched = match_location(&self.routes, self.location());
    }
}

fn validate_route_id(id: &Arc<str>) -> Result<(), RouterError> {
    if id.is_empty() {
        return Err(RouterError::EmptyRouteId);
    }
    if id.len() > MAX_ROUTE_ID_BYTES {
        return Err(RouterError::RouteIdTooLong {
            id: id.clone(),
            limit: MAX_ROUTE_ID_BYTES,
        });
    }
    Ok(())
}

fn resolve_definition(
    index: usize,
    definitions: &[RouteDefinition],
    indices: &HashMap<Arc<str>, usize>,
    marks: &mut [u8],
    resolved: &mut [Option<ResolvedDefinition>],
) -> Result<ResolvedDefinition, RouterError> {
    if marks[index] == 2 {
        return Ok(resolved[index].as_ref().expect("resolved route").clone());
    }
    if marks[index] == 1 {
        return Err(RouterError::ParentCycle {
            id: definitions[index].id.clone(),
        });
    }
    marks[index] = 1;
    let definition = &definitions[index];
    let (parent_pattern, mut route_ids) = if let Some(parent_id) = &definition.parent_id {
        let Some(parent_index) = indices.get(parent_id).copied() else {
            return Err(RouterError::MissingParent {
                route: definition.id.clone(),
                parent: parent_id.clone(),
            });
        };
        let parent = resolve_definition(parent_index, definitions, indices, marks, resolved)?;
        (parent.full_pattern, parent.route_ids.to_vec())
    } else {
        (Arc::from("/"), Vec::new())
    };
    route_ids.push(definition.id.clone());
    if route_ids.len() > MAX_ROUTE_DEPTH {
        return Err(RouterError::RouteTooDeep {
            id: definition.id.clone(),
            limit: MAX_ROUTE_DEPTH,
        });
    }

    let full_pattern = match &definition.path {
        Some(path) if path.starts_with('/') => normalize_pattern(path)?,
        Some(path) if path.is_empty() => parent_pattern,
        Some(path) => normalize_pattern(&format!(
            "{}/{}",
            parent_pattern.trim_end_matches('/'),
            path
        ))?,
        None => parent_pattern,
    };
    let result = ResolvedDefinition {
        full_pattern,
        route_ids: route_ids.into(),
        participates_in_matching: definition.path.is_some(),
    };
    resolved[index] = Some(result.clone());
    marks[index] = 2;
    Ok(result)
}

fn normalize_pattern(pattern: &str) -> Result<Arc<str>, RouterError> {
    if pattern.len() > MAX_ROUTE_PATTERN_BYTES {
        return Err(RouterError::PatternTooLong {
            pattern: Arc::from(pattern),
            limit: MAX_ROUTE_PATTERN_BYTES,
        });
    }
    if pattern.contains('#') {
        return Err(RouterError::InvalidPattern {
            pattern: Arc::from(pattern),
            reason: "fragments belong to destinations, not route patterns",
        });
    }
    let mut normalized = String::new();
    normalized.push('/');
    let mut count = 0;
    for segment in pattern.split('/').filter(|segment| !segment.is_empty()) {
        if segment == "." || segment == ".." {
            return Err(RouterError::InvalidPattern {
                pattern: Arc::from(pattern),
                reason: "`.` and `..` segments are not allowed",
            });
        }
        count += 1;
        if count > MAX_ROUTE_SEGMENTS {
            return Err(RouterError::TooManyPatternSegments {
                pattern: Arc::from(pattern),
                limit: MAX_ROUTE_SEGMENTS,
            });
        }
        if normalized.len() > 1 {
            normalized.push('/');
        }
        normalized.push_str(segment);
    }
    if normalized.len() > MAX_ROUTE_PATTERN_BYTES {
        return Err(RouterError::PatternTooLong {
            pattern: normalized.into(),
            limit: MAX_ROUTE_PATTERN_BYTES,
        });
    }
    Ok(normalized.into())
}

fn compile_pattern(
    pattern: &Arc<str>,
) -> Result<(Vec<PatternSegment>, RouteSpecificity), RouterError> {
    let mut segments = Vec::new();
    let mut names = HashSet::new();
    let mut specificity = RouteSpecificity::default();
    for (index, raw) in pattern
        .split('/')
        .filter(|segment| !segment.is_empty())
        .enumerate()
    {
        let segment = if let Some(name) = raw.strip_prefix(':') {
            let (name, optional) = name
                .strip_suffix('?')
                .map_or((name, false), |name| (name, true));
            validate_parameter_name(pattern, name)?;
            if !names.insert(name.to_owned()) {
                return Err(RouterError::InvalidPattern {
                    pattern: pattern.clone(),
                    reason: "parameter names must be unique",
                });
            }
            if optional {
                specificity.optional_segments += 1;
                PatternSegment::OptionalParameter(Arc::from(name))
            } else {
                specificity.required_segments += 1;
                PatternSegment::Parameter(Arc::from(name))
            }
        } else if let Some(name) = raw.strip_prefix('*') {
            if index + 1
                != pattern
                    .split('/')
                    .filter(|segment| !segment.is_empty())
                    .count()
            {
                return Err(RouterError::InvalidPattern {
                    pattern: pattern.clone(),
                    reason: "a wildcard must be the final segment",
                });
            }
            let name = if name.is_empty() { "*" } else { name };
            if name != "*" {
                validate_parameter_name(pattern, name)?;
            }
            if !names.insert(name.to_owned()) {
                return Err(RouterError::InvalidPattern {
                    pattern: pattern.clone(),
                    reason: "parameter names must be unique",
                });
            }
            specificity.wildcard = true;
            PatternSegment::Wildcard(Arc::from(name))
        } else {
            if raw.contains('*') || raw.contains(':') || raw.contains('?') {
                return Err(RouterError::InvalidPattern {
                    pattern: pattern.clone(),
                    reason: "dynamic markers must begin their segment",
                });
            }
            specificity.static_segments += 1;
            specificity.required_segments += 1;
            PatternSegment::Static(decode_component(raw, false)?)
        };
        segments.push(segment);
    }
    Ok((segments, specificity))
}

fn validate_parameter_name(pattern: &Arc<str>, name: &str) -> Result<(), RouterError> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(RouterError::InvalidPattern {
            pattern: pattern.clone(),
            reason: "parameter names use only letters, numbers, `_`, or `-`",
        });
    }
    Ok(())
}

fn match_location(routes: &[CompiledRoute], location: &RouteLocation) -> Option<RouteMatch> {
    let mut best: Option<(&CompiledRoute, Vec<RouteParameter>)> = None;
    for route in routes {
        let mut params = Vec::new();
        let state_stride = location.decoded_segments.len() + 1;
        let mut failed_states = vec![false; (route.segments.len() + 1) * state_stride];
        if !match_segments(
            &route.segments,
            &location.decoded_segments,
            0,
            0,
            &mut params,
            &mut failed_states,
            state_stride,
        ) {
            continue;
        }
        let replace = best
            .as_ref()
            .is_none_or(|(current, _)| route.specificity.compare(current.specificity).is_gt());
        if replace {
            best = Some((route, params));
        }
    }
    best.map(|(route, params)| RouteMatch {
        route_ids: route.route_ids.clone(),
        params: params.into(),
    })
}

fn match_segments(
    pattern: &[PatternSegment],
    pathname: &[Arc<str>],
    pattern_index: usize,
    pathname_index: usize,
    params: &mut Vec<RouteParameter>,
    failed_states: &mut [bool],
    state_stride: usize,
) -> bool {
    let state_index = pattern_index * state_stride + pathname_index;
    if failed_states[state_index] {
        return false;
    }
    let Some(segment) = pattern.get(pattern_index) else {
        let matched = pathname_index == pathname.len();
        failed_states[state_index] = !matched;
        return matched;
    };
    let matched = match segment {
        PatternSegment::Static(expected) => {
            pathname
                .get(pathname_index)
                .is_some_and(|value| value == expected)
                && match_segments(
                    pattern,
                    pathname,
                    pattern_index + 1,
                    pathname_index + 1,
                    params,
                    failed_states,
                    state_stride,
                )
        }
        PatternSegment::Parameter(name) => {
            if let Some(value) = pathname.get(pathname_index) {
                params.push(RouteParameter {
                    name: name.clone(),
                    value: value.clone(),
                });
                if match_segments(
                    pattern,
                    pathname,
                    pattern_index + 1,
                    pathname_index + 1,
                    params,
                    failed_states,
                    state_stride,
                ) {
                    return true;
                }
                params.pop();
            }
            false
        }
        PatternSegment::OptionalParameter(name) => {
            if let Some(value) = pathname.get(pathname_index) {
                params.push(RouteParameter {
                    name: name.clone(),
                    value: value.clone(),
                });
                if match_segments(
                    pattern,
                    pathname,
                    pattern_index + 1,
                    pathname_index + 1,
                    params,
                    failed_states,
                    state_stride,
                ) {
                    return true;
                }
                params.pop();
            }
            match_segments(
                pattern,
                pathname,
                pattern_index + 1,
                pathname_index,
                params,
                failed_states,
                state_stride,
            )
        }
        PatternSegment::Wildcard(name) => {
            let value = pathname[pathname_index..]
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<&str>>()
                .join("/");
            params.push(RouteParameter {
                name: name.clone(),
                value: Arc::from(value),
            });
            true
        }
    };
    if !matched {
        failed_states[state_index] = true;
    }
    matched
}

fn root_location() -> RouteLocation {
    RouteLocation {
        href: Arc::from("/"),
        pathname: Arc::from("/"),
        search: Arc::from(""),
        hash: Arc::from(""),
        query: Arc::from([]),
        decoded_segments: Arc::from([]),
    }
}

fn resolve_location(
    current: &RouteLocation,
    destination: &str,
) -> Result<RouteLocation, RouterError> {
    if destination.len() > MAX_ROUTE_DESTINATION_BYTES {
        return Err(RouterError::DestinationTooLong {
            limit: MAX_ROUTE_DESTINATION_BYTES,
        });
    }
    if destination.is_empty() {
        return Ok(current.clone());
    }

    let (before_hash, raw_hash) = destination
        .split_once('#')
        .map_or((destination, None), |(before, hash)| (before, Some(hash)));
    let (raw_path, raw_query) = before_hash
        .split_once('?')
        .map_or((before_hash, None), |(path, query)| (path, Some(query)));
    let fragment_only = before_hash.is_empty() && raw_hash.is_some();
    let pathname = if raw_path.is_empty() {
        current.pathname.clone()
    } else {
        Arc::from(resolve_pathname(current.pathname(), raw_path, destination)?)
    };
    let search = if fragment_only {
        current.search.clone()
    } else {
        raw_query
            .filter(|query| !query.is_empty())
            .map_or_else(|| Arc::from(""), |query| Arc::from(format!("?{query}")))
    };
    let hash = raw_hash
        .filter(|hash| !hash.is_empty())
        .map_or_else(|| Arc::from(""), |hash| Arc::from(format!("#{hash}")));
    let href: Arc<str> = format!("{pathname}{search}{hash}").into();
    if href.len() > MAX_ROUTE_DESTINATION_BYTES {
        return Err(RouterError::DestinationTooLong {
            limit: MAX_ROUTE_DESTINATION_BYTES,
        });
    }
    let decoded_segments = decode_path_segments(&pathname)?;
    let query = parse_query(search.strip_prefix('?').unwrap_or(""))?;
    Ok(RouteLocation {
        href,
        pathname,
        search,
        hash,
        query: query.into(),
        decoded_segments: decoded_segments.into(),
    })
}

fn resolve_pathname(current: &str, path: &str, destination: &str) -> Result<String, RouterError> {
    if path.starts_with("//") || looks_like_external_scheme(path) {
        return Err(RouterError::InvalidDestination {
            destination: Arc::from(destination),
            reason: "only internal application paths are supported",
        });
    }
    let mut segments: Vec<&str> = if path.starts_with('/') {
        Vec::new()
    } else {
        current
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect()
    };
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            value => segments.push(value),
        }
        if segments.len() > MAX_ROUTE_SEGMENTS {
            return Err(RouterError::InvalidDestination {
                destination: Arc::from(destination),
                reason: "pathname contains too many segments",
            });
        }
    }
    let mut pathname = String::from("/");
    pathname.push_str(&segments.join("/"));
    Ok(pathname)
}

fn looks_like_external_scheme(path: &str) -> bool {
    let prefix = path.split('/').next().unwrap_or(path);
    let Some((scheme, _)) = prefix.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn decode_path_segments(pathname: &str) -> Result<Vec<Arc<str>>, RouterError> {
    let mut segments = Vec::new();
    for raw in pathname.split('/').filter(|segment| !segment.is_empty()) {
        if segments.len() == MAX_ROUTE_SEGMENTS {
            return Err(RouterError::InvalidDestination {
                destination: Arc::from(pathname),
                reason: "pathname contains too many segments",
            });
        }
        segments.push(decode_component(raw, false)?);
    }
    Ok(segments)
}

fn parse_query(query: &str) -> Result<Vec<RouteQueryPair>, RouterError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let mut pairs = Vec::new();
    for pair in query.split('&') {
        if pairs.len() == MAX_ROUTE_QUERY_PAIRS {
            return Err(RouterError::TooManyQueryPairs {
                limit: MAX_ROUTE_QUERY_PAIRS,
            });
        }
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        pairs.push(RouteQueryPair {
            name: decode_component(name, true)?,
            value: decode_component(value, true)?,
        });
    }
    Ok(pairs)
}

fn decode_component(component: &str, plus_as_space: bool) -> Result<Arc<str>, RouterError> {
    if !component.contains('%') && (!plus_as_space || !component.contains('+')) {
        return Ok(Arc::from(component));
    }
    let bytes = component.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let Some(high) = bytes.get(index + 1).and_then(|byte| hex(*byte)) else {
                    return Err(RouterError::InvalidPercentEncoding {
                        component: Arc::from(component),
                    });
                };
                let Some(low) = bytes.get(index + 2).and_then(|byte| hex(*byte)) else {
                    return Err(RouterError::InvalidPercentEncoding {
                        component: Arc::from(component),
                    });
                };
                decoded.push((high << 4) | low);
                index += 3;
            }
            b'+' if plus_as_space => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded)
        .map(Arc::from)
        .map_err(|_| RouterError::InvalidPercentEncodedUtf8)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn router() -> Router {
        Router::new(
            [
                RouteDefinition::layout("shell"),
                RouteDefinition::new("home", "/").parent("shell"),
                RouteDefinition::new("projects", "/projects").parent("shell"),
                RouteDefinition::new("project", "/projects/:project_id").parent("shell"),
                RouteDefinition::new("settings", "/settings").parent("shell"),
                RouteDefinition::new("settings-index", "").parent("settings"),
                RouteDefinition::new("appearance", "appearance").parent("settings"),
                RouteDefinition::new("optional", "/archive/:year?/:month?").parent("shell"),
                RouteDefinition::new("fallback", "*").parent("shell"),
            ],
            "/",
        )
        .unwrap()
    }

    fn ids(router: &Router) -> Vec<&str> {
        router
            .matched()
            .unwrap()
            .route_ids()
            .iter()
            .map(AsRef::as_ref)
            .collect()
    }

    fn params(router: &Router) -> Vec<(&str, &str)> {
        router
            .matched()
            .unwrap()
            .params()
            .iter()
            .map(|parameter| (parameter.name(), parameter.value()))
            .collect()
    }

    #[test]
    fn matches_static_dynamic_nested_optional_and_fallback_routes() {
        let mut router = router();
        assert_eq!(ids(&router), ["shell", "home"]);

        router.push("/settings").unwrap();
        assert_eq!(ids(&router), ["shell", "settings", "settings-index"]);

        router.push("appearance").unwrap();
        assert_eq!(router.location().pathname(), "/settings/appearance");
        assert_eq!(ids(&router), ["shell", "settings", "appearance"]);

        router.push("/projects/quickgui").unwrap();
        assert_eq!(ids(&router), ["shell", "project"]);
        assert_eq!(params(&router), [("project_id", "quickgui")]);

        router.push("/archive/2026/09").unwrap();
        assert_eq!(ids(&router), ["shell", "optional"]);
        assert_eq!(params(&router), [("year", "2026"), ("month", "09")]);

        router.push("/missing/deep/path").unwrap();
        assert_eq!(ids(&router), ["shell", "fallback"]);
        assert_eq!(params(&router), [("*", "missing/deep/path")]);
    }

    #[test]
    fn static_and_exact_routes_outrank_dynamic_optional_and_wildcard_routes() {
        let router = Router::new(
            [
                RouteDefinition::new("wildcard", "/*rest"),
                RouteDefinition::new("dynamic", "/:name"),
                RouteDefinition::new("optional", "/settings/:panel?"),
                RouteDefinition::new("settings", "/settings"),
            ],
            "/settings",
        )
        .unwrap();
        assert_eq!(ids(&router), ["settings"]);
    }

    #[test]
    fn decodes_params_and_repeated_query_pairs_without_losing_order() {
        let router = Router::new(
            [RouteDefinition::new("project", "/projects/:name")],
            "/projects/Quick%20GUI?tag=native&tag=solid+2&empty=#overview",
        )
        .unwrap();
        assert_eq!(router.location().pathname(), "/projects/Quick%20GUI");
        assert_eq!(router.location().search(), "?tag=native&tag=solid+2&empty=");
        assert_eq!(router.location().hash(), "#overview");
        assert_eq!(params(&router), [("name", "Quick GUI")]);
        assert_eq!(
            router
                .location()
                .query()
                .iter()
                .map(|pair| (pair.name(), pair.value()))
                .collect::<Vec<_>>(),
            [("tag", "native"), ("tag", "solid 2"), ("empty", "")]
        );
    }

    #[test]
    fn push_replace_back_forward_and_forward_truncation_are_exact() {
        let mut router = router();
        router.push("/projects").unwrap();
        router.push("/projects/one?tab=activity").unwrap();
        assert_eq!(router.history_length(), 3);
        assert_eq!(router.history_index(), 2);
        assert!(router.can_go_back());
        assert!(!router.can_go_forward());

        assert!(router.back());
        assert_eq!(router.location().href(), "/projects");
        assert!(router.can_go_forward());
        assert!(router.replace("?sort=recent#list").unwrap());
        assert_eq!(router.location().href(), "/projects?sort=recent#list");
        assert_eq!(router.history_length(), 3);

        router.push("/settings").unwrap();
        assert_eq!(router.history_length(), 3);
        assert!(!router.can_go_forward());
        assert!(!router.forward());
        assert!(router.go(-99));
        assert_eq!(router.location().href(), "/");
        assert!(!router.back());
    }

    #[test]
    fn query_and_fragment_only_destinations_preserve_the_expected_parts() {
        let mut router = router();
        router.push("/projects?sort=name#top").unwrap();
        router.push("#details").unwrap();
        assert_eq!(router.location().href(), "/projects?sort=name#details");
        router.replace("?sort=updated").unwrap();
        assert_eq!(router.location().href(), "/projects?sort=updated");
        assert!(router.is_active("/projects?ignored=yes", true).unwrap());
        assert!(router.is_active("/", false).unwrap());
        assert!(!router.is_active("/projects/one", false).unwrap());
    }

    #[test]
    fn history_stays_bounded() {
        let mut router = router();
        for index in 0..(MAX_ROUTE_HISTORY_ENTRIES + 20) {
            router.push(&format!("/projects/{index}")).unwrap();
        }
        assert_eq!(router.history_length(), MAX_ROUTE_HISTORY_ENTRIES);
        assert_eq!(router.history_index(), MAX_ROUTE_HISTORY_ENTRIES - 1);
        assert!(router.go(isize::MIN));
        assert_eq!(router.location().pathname(), "/projects/20");
    }

    #[test]
    fn optional_parameter_backtracking_has_bounded_state() {
        let optional = (0..32)
            .map(|index| format!("/:value_{index}?"))
            .collect::<String>();
        let pattern = format!("{optional}/expected");
        let pathname = format!("/{}/different", vec!["value"; 16].join("/"));
        let router = Router::new(
            [
                RouteDefinition::new("optional", pattern),
                RouteDefinition::new("fallback", "*"),
            ],
            pathname,
        )
        .unwrap();
        assert_eq!(ids(&router), ["fallback"]);
    }

    #[test]
    fn rejects_invalid_trees_patterns_destinations_and_encoding() {
        assert!(matches!(
            Router::new([RouteDefinition::new("child", "/").parent("missing")], "/"),
            Err(RouterError::MissingParent { .. })
        ));
        assert!(matches!(
            Router::new(
                [
                    RouteDefinition::layout("one").parent("two"),
                    RouteDefinition::layout("two").parent("one"),
                ],
                "/"
            ),
            Err(RouterError::ParentCycle { .. })
        ));
        assert!(matches!(
            Router::new([RouteDefinition::new("bad", "/files/*rest/more")], "/"),
            Err(RouterError::InvalidPattern { .. })
        ));

        let router = router();
        assert!(matches!(
            router.resolve("https://example.com"),
            Err(RouterError::InvalidDestination { .. })
        ));
        assert!(matches!(
            router.resolve("/projects/%zz"),
            Err(RouterError::InvalidPercentEncoding { .. })
        ));
    }
}
