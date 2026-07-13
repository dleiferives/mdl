use std::collections::VecDeque;

use crate::entity::EntityId;

#[cfg(test)]
use super::plan::HomeId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CopyLocation<I> {
    Home(I),
    Scratch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SymbolicMove<I> {
    pub(super) destination: CopyLocation<I>,
    pub(super) source: CopyLocation<I>,
}

/// Resolves simultaneous home assignments in deterministic linear time.
#[cfg(test)]
pub(super) fn resolve_parallel_copies(
    assignments: &[(HomeId, HomeId)],
    home_count: usize,
) -> Result<Vec<SymbolicMove<HomeId>>, ParallelCopyError<HomeId>> {
    ParallelCopyResolver::new().resolve(assignments, home_count)
}

/// Reusable dense scratch for all edge transfers in one planning run.
#[derive(Debug, Default)]
pub(super) struct ParallelCopyResolver {
    destination_seen: Vec<bool>,
    destination_move: Vec<Option<usize>>,
    source_users: Vec<Vec<usize>>,
    source_use_count: Vec<usize>,
    touched_destinations: Vec<usize>,
    touched_sources: Vec<usize>,
}

impl ParallelCopyResolver {
    pub(super) const fn new() -> Self {
        Self {
            destination_seen: Vec::new(),
            destination_move: Vec::new(),
            source_users: Vec::new(),
            source_use_count: Vec::new(),
            touched_destinations: Vec::new(),
            touched_sources: Vec::new(),
        }
    }

    pub(super) fn resolve<I: EntityId + Eq>(
        &mut self,
        assignments: &[(I, I)],
        home_count: usize,
    ) -> Result<Vec<SymbolicMove<I>>, ParallelCopyError<I>> {
        self.prepare(home_count);
        resolve_with_scratch(
            assignments,
            home_count,
            &mut self.destination_seen,
            &mut self.destination_move,
            &mut self.source_users,
            &mut self.source_use_count,
            &mut self.touched_destinations,
            &mut self.touched_sources,
        )
    }

    fn prepare(&mut self, home_count: usize) {
        for index in self.touched_destinations.drain(..) {
            self.destination_seen[index] = false;
            self.destination_move[index] = None;
        }
        for index in self.touched_sources.drain(..) {
            self.source_users[index].clear();
            self.source_use_count[index] = 0;
        }
        self.destination_seen.resize(home_count, false);
        self.destination_move.resize(home_count, None);
        self.source_users.resize_with(home_count, Vec::new);
        self.source_use_count.resize(home_count, 0);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the resolver receives disjoint reusable scratch tables explicitly"
)]
fn resolve_with_scratch<I: EntityId + Eq>(
    assignments: &[(I, I)],
    home_count: usize,
    destination_seen: &mut [bool],
    destination_move: &mut [Option<usize>],
    source_users: &mut [Vec<usize>],
    source_use_count: &mut [usize],
    touched_destinations: &mut Vec<usize>,
    touched_sources: &mut Vec<usize>,
) -> Result<Vec<SymbolicMove<I>>, ParallelCopyError<I>> {
    let mut moves = Vec::with_capacity(assignments.len());
    for (destination, source) in assignments.iter().copied() {
        let destination_index = home_index(destination, home_count)?;
        let source_index = home_index(source, home_count)?;
        if std::mem::replace(&mut destination_seen[destination_index], true) {
            return Err(ParallelCopyError::DuplicateDestination { destination });
        }
        touched_destinations.push(destination_index);
        if destination == source {
            continue;
        }
        let move_index = moves.len();
        destination_move[destination_index] = Some(move_index);
        moves.push(Some((destination, CopyLocation::Home(source))));
        if source_users[source_index].is_empty() {
            touched_sources.push(source_index);
        }
        source_users[source_index].push(move_index);
        source_use_count[source_index] += 1;
    }

    let move_count = moves.len();
    let mut previous = (0..move_count)
        .map(|index| index.checked_sub(1))
        .collect::<Vec<_>>();
    let mut next = (0..move_count)
        .map(|index| (index + 1 < move_count).then_some(index + 1))
        .collect::<Vec<_>>();
    let mut head = (!moves.is_empty()).then_some(0);
    let mut queued = vec![false; move_count];
    let mut ready = VecDeque::new();
    for (index, pending) in moves.iter().enumerate() {
        let destination = pending.expect("move slots begin populated").0;
        if source_use_count[home_index(destination, home_count)?] == 0 {
            ready.push_back(index);
            queued[index] = true;
        }
    }

    let mut output = Vec::with_capacity(move_count + 1);
    let mut remaining = move_count;
    while remaining != 0 {
        if let Some(index) = ready.pop_front() {
            if let Some((destination, source)) = moves[index].take() {
                output.push(SymbolicMove {
                    destination: CopyLocation::Home(destination),
                    source,
                });
                remove_pending(index, &mut previous, &mut next, &mut head);
                remaining -= 1;
                if let CopyLocation::Home(source) = source {
                    let source_index = home_index(source, home_count)?;
                    source_use_count[source_index] -= 1;
                    enqueue_destination_if_ready(
                        source,
                        source_use_count,
                        destination_move,
                        &moves,
                        &mut queued,
                        &mut ready,
                        home_count,
                    )?;
                }
            }
            continue;
        }

        let cycle_move = head.expect("remaining moves keep a pending-list head");
        let cycle_home = moves[cycle_move]
            .expect("pending-list head refers to a move")
            .0;
        output.push(SymbolicMove {
            destination: CopyLocation::Scratch,
            source: CopyLocation::Home(cycle_home),
        });
        let cycle_index = home_index(cycle_home, home_count)?;
        for user in &source_users[cycle_index] {
            let Some((_, source)) = &mut moves[*user] else {
                continue;
            };
            if *source == CopyLocation::Home(cycle_home) {
                *source = CopyLocation::Scratch;
                source_use_count[cycle_index] -= 1;
            }
        }
        enqueue_destination_if_ready(
            cycle_home,
            source_use_count,
            destination_move,
            &moves,
            &mut queued,
            &mut ready,
            home_count,
        )?;
    }
    Ok(output)
}

fn enqueue_destination_if_ready<I: EntityId>(
    home: I,
    source_use_count: &[usize],
    destination_move: &[Option<usize>],
    moves: &[Option<(I, CopyLocation<I>)>],
    queued: &mut [bool],
    ready: &mut VecDeque<usize>,
    home_count: usize,
) -> Result<(), ParallelCopyError<I>> {
    let home_index = home_index(home, home_count)?;
    if source_use_count[home_index] == 0 {
        if let Some(move_index) = destination_move[home_index] {
            if moves[move_index].is_some() && !queued[move_index] {
                queued[move_index] = true;
                ready.push_back(move_index);
            }
        }
    }
    Ok(())
}

fn remove_pending(
    index: usize,
    previous: &mut [Option<usize>],
    next: &mut [Option<usize>],
    head: &mut Option<usize>,
) {
    match previous[index] {
        Some(before) => next[before] = next[index],
        None => *head = next[index],
    }
    if let Some(after) = next[index] {
        previous[after] = previous[index];
    }
}

fn home_index<I: EntityId>(home: I, home_count: usize) -> Result<usize, ParallelCopyError<I>> {
    usize::try_from(home.index())
        .ok()
        .filter(|index| *index < home_count)
        .ok_or(ParallelCopyError::InvalidHome { home })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ParallelCopyError<I> {
    InvalidHome { home: I },
    DuplicateDestination { destination: I },
}

#[cfg(test)]
mod tests {
    use super::{
        CopyLocation, ParallelCopyError, ParallelCopyResolver, SymbolicMove,
        resolve_parallel_copies,
    };
    use crate::entity::EntityId;
    use crate::lower::minecraft::plan::HomeId;

    crate::entity::entity_id!(
        struct AlternateHomeId;
    );

    fn home(index: u32) -> HomeId {
        HomeId::from_index(index)
    }

    fn ordinary(destination: u32, source: u32) -> SymbolicMove<HomeId> {
        SymbolicMove {
            destination: CopyLocation::Home(home(destination)),
            source: CopyLocation::Home(home(source)),
        }
    }

    #[test]
    fn filters_no_ops_and_orders_ready_chains_stably() {
        let moves = resolve_parallel_copies(
            &[(home(0), home(0)), (home(1), home(0)), (home(2), home(1))],
            3,
        )
        .unwrap();
        assert_eq!(moves, vec![ordinary(2, 1), ordinary(1, 0)]);
    }

    #[test]
    fn preserves_fan_out() {
        let moves = resolve_parallel_copies(&[(home(0), home(2)), (home(1), home(2))], 3).unwrap();
        assert_eq!(moves, vec![ordinary(0, 2), ordinary(1, 2)]);
    }

    #[test]
    fn resolves_two_and_three_cycles_with_one_symbolic_scratch() {
        let two = resolve_parallel_copies(&[(home(0), home(1)), (home(1), home(0))], 3).unwrap();
        assert_eq!(two[0].destination, CopyLocation::Scratch);
        assert_eq!(two.len(), 3);

        let three = resolve_parallel_copies(
            &[(home(0), home(1)), (home(1), home(2)), (home(2), home(0))],
            3,
        )
        .unwrap();
        assert_eq!(three[0].destination, CopyLocation::Scratch);
        assert_eq!(three.len(), 4);
    }

    #[test]
    fn mixed_cycles_choose_the_lowest_pending_assignment_stably() {
        let moves = resolve_parallel_copies(
            &[
                (home(0), home(1)),
                (home(1), home(0)),
                (home(2), home(3)),
                (home(3), home(2)),
                (home(4), home(0)),
            ],
            5,
        )
        .unwrap();
        assert_eq!(
            moves
                .iter()
                .filter(|step| step.destination == CopyLocation::Scratch)
                .map(|step| step.source)
                .collect::<Vec<_>>(),
            vec![CopyLocation::Home(home(0)), CopyLocation::Home(home(2))]
        );
    }

    #[test]
    fn rejects_malformed_assignments() {
        assert_eq!(
            resolve_parallel_copies(&[(home(0), home(1)), (home(0), home(2))], 3),
            Err(ParallelCopyError::DuplicateDestination {
                destination: home(0)
            })
        );
        assert_eq!(
            resolve_parallel_copies(&[(home(3), home(0))], 3),
            Err(ParallelCopyError::InvalidHome { home: home(3) })
        );
    }

    #[test]
    fn reusable_scratch_resets_after_success_and_failure() {
        let mut resolver = ParallelCopyResolver::new();
        let cycle = resolver
            .resolve(&[(home(0), home(1)), (home(1), home(0))], 2)
            .unwrap();
        assert_eq!(cycle.len(), 3);
        assert!(matches!(
            resolver.resolve(&[(home(0), home(1)), (home(0), home(0))], 2),
            Err(ParallelCopyError::DuplicateDestination { .. })
        ));

        assert_eq!(
            resolver.resolve(&[(home(1), home(0))], 2).unwrap(),
            vec![ordinary(1, 0)]
        );
    }

    #[test]
    fn reusable_resolver_accepts_a_second_typed_id_without_conversion() {
        let mut resolver = ParallelCopyResolver::new();
        let alternate = |index| AlternateHomeId::from_index(index);
        assert_eq!(
            resolver.resolve(&[(home(1), home(0))], 2).unwrap(),
            vec![ordinary(1, 0)]
        );

        let moves = resolver
            .resolve(
                &[(alternate(0), alternate(1)), (alternate(1), alternate(0))],
                2,
            )
            .unwrap();

        assert_eq!(
            moves,
            vec![
                SymbolicMove {
                    destination: CopyLocation::Scratch,
                    source: CopyLocation::Home(alternate(0)),
                },
                SymbolicMove {
                    destination: CopyLocation::Home(alternate(0)),
                    source: CopyLocation::Home(alternate(1)),
                },
                SymbolicMove {
                    destination: CopyLocation::Home(alternate(1)),
                    source: CopyLocation::Scratch,
                },
            ]
        );
        assert_eq!(
            resolver.resolve(&[(alternate(2), alternate(0))], 2),
            Err(ParallelCopyError::InvalidHome { home: alternate(2) })
        );
    }
}
