use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{
    Attribute, FnArg, Ident, ImplItem, ImplItemFn, ItemImpl, LitInt, LitStr, Pat, Result, Token,
};

use crate::{parse_optional_comma, push_error};

pub(crate) fn expand_schedule(mut item_impl: ItemImpl) -> Result<proc_macro2::TokenStream> {
    if item_impl.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &item_impl,
            "#[schedule] can only be used on inherent impl blocks",
        ));
    }

    let self_ty = item_impl.self_ty.clone();
    let mut jobs = Vec::new();
    let mut errors: Option<syn::Error> = None;

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (clean_attrs, specs, schedule_errors) = take_schedule_job_attrs(&method.attrs);
        method.attrs = clean_attrs;
        for error in schedule_errors {
            push_error(&mut errors, error);
        }
        if specs.is_empty() {
            continue;
        }

        let input = match ScheduleMethodInput::from_method(method) {
            Ok(input) => input,
            Err(error) => {
                push_error(&mut errors, error);
                continue;
            }
        };

        if method.sig.asyncness.is_none() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(method.sig.fn_token, "scheduled job methods must be async"),
            );
            continue;
        }

        for spec in specs {
            match schedule_job_registration(method, &input, spec) {
                Ok(job) => jobs.push(job),
                Err(error) => push_error(&mut errors, error),
            }
        }
    }

    if let Some(error) = errors {
        return Err(error);
    }

    Ok(quote! {
        #item_impl

        impl #self_ty {
            pub fn scheduled_jobs(
                self: ::std::sync::Arc<Self>,
            ) -> ::std::vec::Vec<::a3s_boot::ScheduledJob> {
                let mut __a3s_boot_jobs = ::std::vec::Vec::new();
                #(
                    __a3s_boot_jobs.push(#jobs);
                )*
                __a3s_boot_jobs
            }
        }
    })
}

fn take_schedule_job_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<ScheduleJobSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut specs = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = ScheduleJobKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match kind.parse_args(attr) {
            Ok(args) => specs.push(ScheduleJobSpec { kind, args }),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, specs, errors)
}

fn schedule_job_registration(
    method: &ImplItemFn,
    input: &ScheduleMethodInput,
    spec: ScheduleJobSpec,
) -> Result<proc_macro2::TokenStream> {
    let method_ident = &method.sig.ident;
    let name = spec.args.name_token(method_ident);
    let handler = scheduled_task_handler(method_ident, input);

    Ok(match spec.kind {
        ScheduleJobKind::Cron => {
            let ScheduleJobArgs::Cron(args) = spec.args else {
                unreachable!("cron schedule spec must use cron args")
            };
            let expression = args.expression;
            quote! {
                ::a3s_boot::ScheduledJob::cron(#name, #expression, #handler)
            }
        }
        ScheduleJobKind::Interval => {
            let ScheduleJobArgs::Interval(args) = spec.args else {
                unreachable!("interval schedule spec must use interval args")
            };
            let millis = args.duration_millis()?;
            quote! {
                ::a3s_boot::ScheduledJob::interval(
                    #name,
                    ::std::time::Duration::from_millis(#millis),
                    #handler
                )
            }
        }
        ScheduleJobKind::Timeout => {
            let ScheduleJobArgs::Timeout(args) = spec.args else {
                unreachable!("timeout schedule spec must use timeout args")
            };
            let millis = args.duration_millis()?;
            quote! {
                ::a3s_boot::ScheduledJob::timeout(
                    #name,
                    ::std::time::Duration::from_millis(#millis),
                    #handler
                )
            }
        }
    })
}

fn scheduled_task_handler(
    method_ident: &Ident,
    input: &ScheduleMethodInput,
) -> proc_macro2::TokenStream {
    let scheduled_name = format_ident!("__a3s_boot_scheduled_{}", method_ident);
    let (closure_arg, method_args) = if input.accepts_context {
        (quote!(__a3s_boot_context), quote!(__a3s_boot_context))
    } else {
        (quote!(_context), quote!())
    };

    quote! {
        {
            let #scheduled_name = ::std::sync::Arc::clone(&self);
            move |#closure_arg: ::a3s_boot::ScheduleContext| {
                let #scheduled_name = ::std::sync::Arc::clone(&#scheduled_name);
                async move { #scheduled_name.#method_ident(#method_args).await }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ScheduleJobKind {
    Cron,
    Interval,
    Timeout,
}

impl ScheduleJobKind {
    fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "cron" => Some(Self::Cron),
            "interval" => Some(Self::Interval),
            "timeout" => Some(Self::Timeout),
            _ => None,
        }
    }

    fn parse_args(self, attr: &Attribute) -> Result<ScheduleJobArgs> {
        match self {
            Self::Cron => attr
                .parse_args::<CronScheduleArgs>()
                .map(ScheduleJobArgs::Cron),
            Self::Interval => attr
                .parse_args::<DurationScheduleArgs>()
                .map(ScheduleJobArgs::Interval),
            Self::Timeout => attr
                .parse_args::<DurationScheduleArgs>()
                .map(ScheduleJobArgs::Timeout),
        }
    }
}

struct ScheduleJobSpec {
    kind: ScheduleJobKind,
    args: ScheduleJobArgs,
}

enum ScheduleJobArgs {
    Cron(CronScheduleArgs),
    Interval(DurationScheduleArgs),
    Timeout(DurationScheduleArgs),
}

impl ScheduleJobArgs {
    fn name_token(&self, method_ident: &Ident) -> proc_macro2::TokenStream {
        let explicit_name = match self {
            Self::Cron(args) => args.name.as_ref(),
            Self::Interval(args) | Self::Timeout(args) => args.name.as_ref(),
        };

        match explicit_name {
            Some(name) => quote!(#name),
            None => {
                let default_name = LitStr::new(&method_ident.to_string(), method_ident.span());
                quote!(#default_name)
            }
        }
    }
}

struct CronScheduleArgs {
    name: Option<LitStr>,
    expression: LitStr,
}

impl Parse for CronScheduleArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let first = input.parse::<LitStr>()?;
        if input.is_empty() {
            return Ok(Self {
                name: None,
                expression: first,
            });
        }

        input.parse::<Token![,]>()?;
        let expression = input.parse::<LitStr>()?;
        parse_optional_comma(input)?;
        Ok(Self {
            name: Some(first),
            expression,
        })
    }
}

struct DurationScheduleArgs {
    name: Option<LitStr>,
    millis: LitInt,
}

impl DurationScheduleArgs {
    fn duration_millis(&self) -> Result<u64> {
        self.millis.base10_parse::<u64>()
    }
}

impl Parse for DurationScheduleArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let (name, millis) = if input.peek(LitInt) {
            (None, input.parse::<LitInt>()?)
        } else if input.peek(LitStr) {
            let name = input.parse::<LitStr>()?;
            input.parse::<Token![,]>()?;
            (Some(name), input.parse::<LitInt>()?)
        } else {
            return Err(input.error("expected milliseconds or a job name followed by milliseconds"));
        };

        parse_optional_comma(input)?;
        Ok(Self { name, millis })
    }
}

#[derive(Clone, Copy)]
struct ScheduleMethodInput {
    accepts_context: bool,
}

impl ScheduleMethodInput {
    fn from_method(method: &ImplItemFn) -> Result<Self> {
        let mut inputs = method.sig.inputs.iter();
        let Some(FnArg::Receiver(receiver)) = inputs.next() else {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                "scheduled job methods must take &self as their first argument",
            ));
        };

        if receiver.reference.is_none() || receiver.mutability.is_some() {
            return Err(syn::Error::new_spanned(
                receiver,
                "scheduled job methods must use an immutable &self receiver",
            ));
        }

        let mut accepts_context = false;
        for (index, input) in inputs.enumerate() {
            let FnArg::Typed(input) = input else {
                return Err(syn::Error::new_spanned(
                    input,
                    "unexpected receiver argument",
                ));
            };

            if index > 0 {
                return Err(syn::Error::new_spanned(
                    input,
                    "scheduled job methods can accept at most one ScheduleContext argument after &self",
                ));
            }

            let Pat::Ident(_) = input.pat.as_ref() else {
                return Err(syn::Error::new_spanned(
                    &input.pat,
                    "scheduled job arguments must be simple identifiers",
                ));
            };
            accepts_context = true;
        }

        Ok(Self { accepts_context })
    }
}
