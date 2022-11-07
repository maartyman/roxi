use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Error;
use std::iter::empty;
use std::rc::Rc;
use oxigraph::sparql::Variable;
use spargebra::Query;
use spargebra::Query::Select;
use spargebra::algebra::*;
use spargebra::term::TriplePattern;
use crate::{Encoder, Parser, Syntax, TermImpl, Triple, TripleIndex, TripleStore, VarOrTerm};
use crate::sparql::EncodedTerm::NamedNode;
use crate::sparql::PlanNode::QuadPattern;
use crate::tripleindex::EncodedBinding;


fn extract_triples(triple_patterns: &Vec<TriplePattern>, encoder: &mut Encoder)-> Vec<Triple>{
    let mut triples = Vec::new();
    println!("BGP: {:?}", triple_patterns);
    for TriplePattern{subject: s , predicate: p,object:o } in triple_patterns{
        println!("subject: {:?}", s.to_string());
        println!("predicate: {:?}", p.to_string());
        println!("object: {:?}", o.to_string());
        triples.push(Triple::from(s.to_string(),p.to_string(),o.to_string(), encoder));
    }
    triples
}
#[derive(Debug)]
pub enum PlanExpression{
    Constant(TermImpl),
    Variable(usize),
    Greater(Box<Self>, Box<Self>),
    GreaterOrEqual(Box<Self>, Box<Self>),
    Less(Box<Self>, Box<Self>),
    LessOrEqual(Box<Self>, Box<Self>),
    Done
}
#[derive(Debug)]
pub enum PlanNode{
    Join{left: Box<Self>, right: Box<Self>},
    QuadPattern{pattern: Triple},
    Project {
        child: Box<Self>,
        mapping: Vec<usize>,
    },
    Filter{
        child: Box<Self>,
        expression: Box<PlanExpression>,
    },
    Aggregate {
        // By definition the group by key are the range 0..key_mapping.len()
        child: Box<Self>,
        keys: Vec<Variable>, // aggregate key pairs of (variable key in child, variable key in output)
        aggregates: Rc<Vec<(PlanAggregation, Variable)>>,
    },
    Extend{
        child: Box<Self>,
        from: Variable,
        to: Variable
    },
    Done
}
#[derive(Eq, PartialEq, Debug, Clone, Hash)]
pub struct PlanAggregation {
    pub function: PlanAggregationFunction,
    pub distinct: bool,
}

#[derive(Eq, PartialEq, Debug, Clone, Hash)]
pub enum PlanAggregationFunction {
    Count,
    Sum,
    Min,
    Max,
    Avg
}
fn new_join(left: PlanNode, right: PlanNode) -> PlanNode{
    PlanNode::Join {left:Box::new(left),right: Box::new(right)}
}
fn extract_query_plan(graph_pattern: &GraphPattern, encoder: &mut Encoder) -> PlanNode {
    match graph_pattern {
        GraphPattern::Bgp {patterns}=> patterns.iter().map(|t| QuadPattern {pattern:Triple::from(t.subject.to_string(),t.predicate.to_string(),t.object.to_string(), encoder)}).
            reduce(new_join).unwrap(),
        GraphPattern::Project {inner,variables}=>{
            PlanNode::Project {child: Box::new(extract_query_plan(inner,encoder)), mapping: variables.iter().map(|v|encoder.add(v.as_str().to_string())).collect()}
        },
        GraphPattern::Filter {expr, inner} =>{
            println!("Expression: {:?}",expr);
            println!("inner: {:?}",inner);
            PlanNode::Filter{child: Box::new(extract_query_plan(inner, encoder)), expression: Box::new(extract_expression(expr, encoder))}
        },
        GraphPattern::Group {
            inner,
            variables: by,
            aggregates,
        } => {
            let mut inner_variables = by.clone();
            println!(" othere vars {:?}", aggregates);
            println!("  vars {:?}", by);

            PlanNode::Aggregate {
                child: Box::new(extract_query_plan(
                    inner,
                    encoder,
                )),
                keys: inner_variables.clone(),
                aggregates: Rc::new(
                    aggregates
                        .iter()
                        .map(|(v, a)| {
                            Ok((
                                build_for_aggregate(a, &mut inner_variables).unwrap(),
                                v.clone(),
                            ))
                        })
                        .collect::<Result<Vec<_>, Error>>().unwrap(),
                ),
            }
        },
        GraphPattern::Extend {inner, expression,variable } => {
            if let Expression::Variable(var_exp) = expression{
                encoder.add(var_exp.clone().into_string());

                encoder.add(variable.clone().into_string());
                PlanNode::Extend{child:Box::new(extract_query_plan(inner,encoder)) , from: var_exp.clone(), to: variable.clone()}
            }else{
                PlanNode::Done
            }
        }
        _ => PlanNode::Done,
    }
}
fn build_for_aggregate(
    aggregate: &AggregateExpression,
    variables: &mut Vec<Variable>
) -> Result<PlanAggregation, String> {
    match aggregate {
        AggregateExpression::Count { expr, distinct } => Ok(PlanAggregation {
            function: PlanAggregationFunction::Count,
            distinct: *distinct,
        }),

        _ => {Err("Failed".to_string())}
    }
}

fn extract_expression(expression: &Expression,encoder: &mut Encoder) -> PlanExpression {
    match expression {
        Expression::Greater(a,b)=>{
            PlanExpression::Greater(Box::new(extract_expression(a, encoder)),Box::new(extract_expression(b, encoder)))
        },
        Expression::GreaterOrEqual(a,b)=>{
            PlanExpression::GreaterOrEqual(Box::new(extract_expression(a, encoder)),Box::new(extract_expression(b, encoder)))
        },
        Expression::Less(a,b)=>{
            PlanExpression::Less(Box::new(extract_expression(a, encoder)),Box::new(extract_expression(b, encoder)))
        },
        Expression::LessOrEqual(a,b)=>{
            PlanExpression::LessOrEqual(Box::new(extract_expression(a, encoder)),Box::new(extract_expression(b, encoder)))
        },
        Expression::Variable(var)=>PlanExpression::Variable(encoder.add(var.as_str().to_string())),
        Expression::Literal(value)=>PlanExpression::Constant(TermImpl{iri:value.value().parse::<usize>().unwrap()}),
        _=> PlanExpression::Done

    }
}
#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub struct Binding{
    pub var: String,
    pub val: String
}

fn decode(input: &EncodedBinding, encoder: &Encoder) -> Binding{
    Binding{var: encoder.decode(&input.var).unwrap_or(&"".to_string()).clone(),
        val: encoder.decode(&input.val).unwrap_or(&"".to_string()).clone()}
}
pub fn evaluate_plan_and_debug<'a>(plan_node: &'a PlanNode, triple_index: &'a TripleIndex, encoder: &'a  Encoder) -> Box<dyn Iterator<Item=Vec<Binding>> + 'a> {
    Box::new(evaluate_plan(plan_node,triple_index,encoder).map(|v|v.into_iter().map(|b|decode(&b,encoder)).collect::<Vec<Binding>>()))
}
pub fn evaluate_plan<'a>(plan_node: &'a PlanNode, triple_index: &'a TripleIndex, encoder: &'a  Encoder) -> Box<dyn Iterator<Item=Vec<EncodedBinding>> + 'a> {
    match plan_node{
        PlanNode::QuadPattern {pattern: triple}=>{

            triple_index.query_help(&triple,None)
        },
        PlanNode::Project {child,mapping}=>{
            let child_it = evaluate_plan(child, triple_index,encoder);

            Box::new(child_it.map(|binding|{
                let projection : Vec<EncodedBinding>= binding.into_iter().filter(|b|mapping.contains(&b.var)).collect();
                projection
            }))
        },
        PlanNode::Join {left, right}=> {
            let left = evaluate_plan(left,triple_index, encoder);
            let right = evaluate_plan(right,triple_index, encoder);
            let mut left = left.peekable();
            let mut right = right.peekable();
            let left_peek = left.peek();
            let right_peek = right.peek();
            if let (Some(left_bindings),Some(right_bindings)) = (left_peek,right_peek){
                let left_vars: Vec<usize> = left_bindings.iter().map(|b|b.var).collect();
                let intersection: Vec<usize> = right_bindings.iter().filter(|b|left_vars.contains(&b.var)).map(|b|b.var).collect();
                //create the hash for left side
                let mut hash = HashMap::new();
                left.into_iter().for_each(|bindings|{
                    for binding in &bindings{
                        if binding.var == *intersection.get(0).unwrap(){
                            if ! hash.contains_key(&binding.val){
                                hash.insert(binding.val, Vec::new());
                            }
                            hash.get_mut(&binding.val).unwrap().push(bindings.clone());
                        }
                    }
                });
                return Box::new(right.map(move |bindings|{
                    let mut all_bindings = Vec::new();
                    for binding in &bindings{
                        if binding.var == *intersection.get(0).unwrap(){
                            if hash.contains_key(&binding.val){

                                for hashed in  hash.get(&binding.val).unwrap(){
                                    let mut new_bindings = bindings.clone();
                                    new_bindings.append(&mut hashed.clone());
                                    all_bindings.push(new_bindings);
                                }


                            }
                        }
                    }
                    all_bindings
                }).flatten());
            }
            Box::new(empty())},
        PlanNode::Filter {child, expression}=>{
            let child = evaluate_plan(child,triple_index, encoder);
            let expression = eval_expression(expression, encoder);
            Box::new(child.filter(move|bindings|{
                if let Some(EncodedTerm::BooleanLiteral(true)) = expression(bindings){
                    true
                }else{
                    false
                }
            }))
            },
        PlanNode::Aggregate {child,keys,aggregates}=>{
            let child = evaluate_plan(child,triple_index, encoder);
            println!("keeys: {:?}", keys);
                let (aggregate_function, aggregate_var) = aggregates.iter().next().unwrap();
            //TODO match aggregate function
            println!("aggregate var: {:?}", aggregate_var.clone().into_string());
            let aggregate_encoded = encoder.get(aggregate_var.as_str()).unwrap();
            println!("encoded aggregate var: {:?}", aggregate_encoded);

            let mut grouped_accumulators =
                    Rc::new(RefCell::new(HashMap::<Vec<usize>, CountAccumulator>::default()));
            let  local_group = grouped_accumulators.clone();
                child.for_each(move |child_binding| {
                    let key_values : Vec<usize> = keys.iter().map(|v| encoder.get(v.as_str()).unwrap()).collect();
                    println!("encoded keys {:?}", key_values);
                    let mut converted_keys = Vec::with_capacity(key_values.len());
                    for key_val in key_values{
                        for binding in child_binding.clone(){
                            if key_val == binding.var {
                                println!("encoded var {:?} decoded: {:?}", binding.val, encoder.decode(&binding.val));
                                converted_keys.push(binding.val)
                            }
                        }
                    }
                    {
                        let mut temp_acc = grouped_accumulators.borrow_mut();
                        let t = temp_acc.entry(converted_keys).or_insert_with(|| CountAccumulator::default());
                        t.add(child_binding);
                    }
                    println!("Groups {:?}", grouped_accumulators);

                }


                );
                //build a new set of bindings
            {
                let mut temp_acc = local_group.borrow_mut();
                let mut new_bindings = Vec::with_capacity(temp_acc.len());
                let key_values : Vec<usize> = keys.iter().map(|v| encoder.get(v.as_str()).unwrap()).collect();

                for (group_keys, group_value) in temp_acc.iter(){
                    let mut new_row = Vec::with_capacity(key_values.len()+1);
                    let binding = EncodedBinding{var: aggregate_encoded, val: group_value.get() };
                    new_row.push(binding);
                    for (i,key_val) in key_values.clone().into_iter().enumerate(){
                        let binding = EncodedBinding{var: key_val, val: group_keys.get(i).unwrap().clone() };
                        new_row.push(binding);
                    }
                    new_bindings.push(new_row);
                }
                println!("New bindings: {:?}", new_bindings);
                Box::new(new_bindings.into_iter())
            }




        },
        PlanNode::Extend {child, from, to}=>{
            let child_it = evaluate_plan(child, triple_index,encoder);
            let encoded_from = encoder.get(from.as_str()).unwrap();
            let encoded_to = encoder.get(to.as_str()).unwrap();
            Box::new(child_it.map(move |binding|{
                let projection : Vec<EncodedBinding>= binding.into_iter().map(|b|{
                    if b.var == encoded_from{
                        EncodedBinding{var: encoded_to, ..b}
                    }else{
                        b
                    }
                }).collect();
                projection
            }))
        }
        PlanNode::Done => Box::new(empty())
    }
}
#[derive(Debug)]
pub struct CountAccumulator{
    count: usize
}
impl CountAccumulator{
    fn add(&mut self, _item: Vec<EncodedBinding>){
        self.count+=1;
    }
    fn get(&self) -> usize{
        self.count
    }
}
impl Default for CountAccumulator{
    fn default() -> Self {
        CountAccumulator{count: 0}
    }
}
fn eval_expression<'a>(expression: &'a PlanExpression, encoder: &'a Encoder) ->  Box<dyn Fn(&Vec<EncodedBinding>) -> Option<EncodedTerm> + 'a>{
    match expression{
        PlanExpression::Greater(a,b)=>{
            partial_compare_helper(encoder, a, b, Ordering::Greater, None)
        },
        PlanExpression::Less(a,b)=>{
            partial_compare_helper(encoder, a, b, Ordering::Less, None)
        },
        PlanExpression::GreaterOrEqual(a,b)=>{
            partial_compare_helper(encoder, a, b, Ordering::Greater, Some(Ordering::Equal))
        },
        PlanExpression::LessOrEqual(a,b)=>{
            partial_compare_helper(encoder, a, b, Ordering::Less, Some(Ordering::Equal))
        },
        PlanExpression::Variable(v)=> Box::new(move |bindings|{
            let var_value : Vec<&EncodedBinding> = bindings.iter().filter(|b|b.var==*v).collect();
            var_value.get(0).iter().map(|v|{
                if let  Some(decoded) = encoder.decode(&v.val){
                    let split: Vec<&str> = decoded.split("^^").collect();
                    if let Some(iri) = split.get(1){
                        let value = split.get(0).unwrap();
                        let value = &value[1..value.len()-1];
                        match iri{
                            &"<http://www.w3.org/2001/XMLSchema#integer>"=>return value.parse::<usize>().unwrap().into(),
                            _ =>  return v.val.into()
                        }
                    }

                };
                v.val.into()
            }).next()
        }),
        PlanExpression::Constant(t) => {
            let t = t.clone();
            Box::new(move |_| Some(EncodedTerm::IntegerLiteral (t.iri.clone())))
        },
        _ => Box::new(|bindings|Some(false.into()))
    }

}

fn partial_compare_helper<'a>(encoder: &'a Encoder, a: &'a Box<PlanExpression>, b: &'a Box<PlanExpression>, ordering: Ordering, second_order: Option<Ordering>) -> Box<dyn Fn(&Vec<EncodedBinding>) -> Option<EncodedTerm> +'a> {
    let a = eval_expression(a, encoder);
    let b = eval_expression(b, encoder);

    Box::new(move |bindings| {
        let b_res = b(bindings);

        let r: Option<Ordering> = match a(bindings) {
            Some(EncodedTerm::IntegerLiteral(int_val_a)) => match b_res {
                Some(EncodedTerm::IntegerLiteral(int_val_b)) => int_val_a.partial_cmp(&int_val_b).into(),
                _ => None
            },
            Some(EncodedTerm::StringLiteral(str_val_a)) => match b(bindings) {
                Some(EncodedTerm::StringLiteral(str_val_b)) => str_val_a.partial_cmp(&str_val_b),
                _ => None
            },
            _ => None
        };
        let r = r.unwrap();
        if let Some(second_ordering) = second_order{
            if r == ordering  || r == second_ordering{
                Some(true.into())
            }else{
                Some(false.into())
            }

        }else{
            Some((r == ordering).into())
        }
    })
}

fn to_bool(term: &EncodedTerm) -> Option<bool> {
    match term {
        EncodedTerm::BooleanLiteral(value) => Some(*value),
        EncodedTerm::StringLiteral(value) => Some(!value.is_empty()),
        EncodedTerm::IntegerLiteral(value) => Some(*value != 0),
        _ => None,
    }
}
pub enum EncodedTerm{
    NamedNode {
        iri_id: usize,
    },
    StringLiteral(String),
    IntegerLiteral(usize),
    BooleanLiteral(bool)
}
impl From<bool> for EncodedTerm {
    fn from(value: bool) -> Self {
        Self::BooleanLiteral(value)
    }
}
impl From<String> for EncodedTerm {
    fn from(value: String) -> Self {
        Self::StringLiteral(value)
    }
}
impl From<usize> for EncodedTerm {
    fn from(value: usize) -> Self {
        Self::IntegerLiteral(value)
    }
}
pub struct QueryResults {
    plan: PlanNode,
    iterator: Box<dyn Iterator<Item=Vec<EncodedBinding>>>
}
impl Iterator for QueryResults{
    type Item = Vec<EncodedBinding>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iterator.next()
    }
}
pub fn eval_query<'a>(query: &'a Query, index: &'a TripleIndex, encoder: &'a mut Encoder) -> PlanNode {
    match query {
        spargebra::Query::Select {
            pattern, base_iri, ..
        } => {
            println!("Select query");
            let plan = extract_query_plan(&pattern,encoder);

            plan
        }
        spargebra::Query::Ask {
            pattern, base_iri, ..
        } => {
            println!("Ask query");
            PlanNode::Done
        }
        spargebra::Query::Construct {
            template,
            pattern,
            base_iri,
            ..
        } => {
            println!("Construct query");
            PlanNode::Done }
        spargebra::Query::Describe {
            pattern, base_iri, ..
        } => {
            println!("Describe query");
            PlanNode::Done        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn prepare_test() -> (TripleIndex, Encoder) {
        //load triples
        let nquads = "<http://example.com/foo> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://schema.org/Person> <http://example.com/> .
    <http://example.com/foo> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://schema.org/Student> <http://example.com/somethingelse> .
    <http://example.com/foo> <http://test/hasVal> \"1\"^^<http://www.w3.org/2001/XMLSchema#integer> <http://example.com/somethingelse> .
    <http://example.com/foo2> <http://test/hasVal> \"10\"^^<http://www.w3.org/2001/XMLSchema#integer> <http://example.com/somethingelse> .";

        let mut encoder = Encoder::new();
        let triples = Parser::parse_triples(nquads, &mut encoder, Syntax::NQuads).unwrap();
        let mut index = TripleIndex::new();
        triples.into_iter().for_each(|t| index.add(t));
        (index, encoder)
    }
    #[test]
    fn test_filter_greater_than() {
        let (index, mut encoder) = prepare_test();
        let query_str = "Select * WHERE {  ?s <http://test/hasVal> ?o2  . Filter(?o2 > 1). }";
        let query = Query::parse(query_str, None).unwrap();
        let plan = eval_query(&query, &index, &mut encoder);
        let iterator = evaluate_plan(&plan, &index, & encoder);
        assert_eq!(1, iterator.collect::<Vec<Vec<EncodedBinding>>>().len());
    }
    #[test]
    fn test_filter_greater_than_or_equal() {
        let (index, mut encoder) = prepare_test();
        let query_str = "Select * WHERE {  ?s <http://test/hasVal> ?o2  . Filter(?o2 >= 1). }";
        let query = Query::parse(query_str, None).unwrap();
        let plan = eval_query(&query, &index, &mut encoder);
        let iterator = evaluate_plan(&plan, &index, & encoder);
        assert_eq!(2, iterator.collect::<Vec<Vec<EncodedBinding>>>().len());
    }
    #[test]
    fn test_filter_less_than() {
        let (index, mut encoder) = prepare_test();
        let query_str = "Select * WHERE {  ?s <http://test/hasVal> ?o2  . Filter(?o2 <= 1). }";
        let query = Query::parse(query_str, None).unwrap();
        let plan = eval_query(&query, &index, &mut encoder);
        let iterator = evaluate_plan(&plan, &index, & encoder);
        assert_eq!(1, iterator.collect::<Vec<Vec<EncodedBinding>>>().len());
    }
    #[test]
    fn test_group_by_count_aggregation() {
        let (index, mut encoder) = prepare_test();
        let query_str = "Select (COUNT(?s) AS ?count) ?o2 WHERE {  ?s <http://test/hasVal> ?o2  .  } GroupBy ?s ?o2";
        let query = Query::parse(query_str, None).unwrap();
        println!("{:?}", query);
        let plan = eval_query(&query, &index, &mut encoder);
        let iterator = evaluate_plan_and_debug(&plan, &index, & encoder);
        for r in iterator{
            println!("result: {:?}", r);
        }
        //assert_eq!(1, iterator.collect::<Vec<Vec<EncodedBinding>>>().len());
    }
}