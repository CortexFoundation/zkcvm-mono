use zkvm_opcodes::{
    system_params::{EVENT_AUX_BYTE, L1_MESSAGE_AUX_BYTE, PRECOMPILE_AUX_BYTE, STORAGE_AUX_BYTE},
    LogOpcode, Opcode, PrecompileCallABI, FIRST_MESSAGE_FLAG_IDX,
};
use zkvm_primitives::queries::LogQuery;

use super::*;

impl<const N: usize, E: VmEncodingMode<N>> DecodedOpcode<N, E> {
    pub fn log_opcode_apply<
        S: zkvm_primitives::vm::Storage,
        M: zkvm_primitives::vm::Memory,
        EV: zkvm_primitives::vm::EventSink,
        PP: zkvm_primitives::vm::PrecompilesProcessor,
        DP: zkvm_primitives::vm::DecommittmentProcessor,
        WT: crate::witness_trace::VmWitnessTracer<N, E>,
    >(
        &self,
        vm_state: &mut VmState<S, M, EV, PP, DP, WT, N, E>,
        prestate: PreState<N, E>,
    ) {
        let PreState { src0, src1, dst0_mem_location, new_pc, .. } = prestate;
        let PrimitiveValue { value: src0, is_pointer: _ } = src0;
        let PrimitiveValue { value: src1, is_pointer: _ } = src1;
        let inner_variant = match self.variant.opcode {
            Opcode::Log(inner) => inner,
            _ => unreachable!(),
        };
        vm_state.local_state.callstack.get_current_stack_mut().pc = new_pc;
        let is_first_message = self.variant.flags[FIRST_MESSAGE_FLAG_IDX];

        // this is the only case where we do extra checking for costs as it's related to pubdata
        // and shard_id

        // We do it as the following:
        // - check if we have enough
        // - if not - just set remaining ergs to 0 (that will cause an exception on the next cycle)
        // - DO NOT set any pending
        // - return

        // ergs exception handling
        let shard_id = vm_state
            .local_state
            .callstack
            .get_current_stack()
            .this_shard_id;
        let ergs_available = vm_state
            .local_state
            .callstack
            .get_current_stack()
            .ergs_remaining;
        let is_rollup = shard_id == 0;

        let timestamp_for_log = vm_state.timestamp_for_first_decommit_or_precompile_read();
        let tx_number_in_block = vm_state.local_state.tx_number_in_block;

        let ergs_on_pubdata = match inner_variant {
            LogOpcode::StorageWrite => {
                let key = src0;
                let written_value = src1;

                let current_context = vm_state.local_state.callstack.get_current_stack();
                let address = current_context.this_address;
                let shard_id = current_context.this_shard_id;

                #[allow(dropping_references)]
                drop(current_context);

                // we do not need all the values here, but we DO need the written value
                // for oracle to do estimations

                let partial_query = LogQuery {
                    timestamp: timestamp_for_log,
                    tx_number_in_block,
                    aux_byte: STORAGE_AUX_BYTE,
                    shard_id,
                    address,
                    key,
                    read_value: U256::ZERO,
                    written_value,
                    rw_flag: true,
                    rollback: false,
                    is_service: false,
                };

                let refund = vm_state.refund_for_partial_query(
                    vm_state.local_state.monotonic_cycle_counter,
                    &partial_query,
                );
                let pubdata_refund = refund.pubdata_refund();

                let net_pubdata = if is_rollup {
                    let (net_cost, uf) =
                        (zkvm_opcodes::system_params::INITIAL_STORAGE_WRITE_PUBDATA_BYTES as u32)
                            .overflowing_sub(pubdata_refund);
                    assert!(!uf, "refund can not be more than net cost itself");

                    net_cost
                } else {
                    assert_eq!(pubdata_refund, 0);

                    0
                };

                vm_state.local_state.current_ergs_per_pubdata_byte * net_pubdata
            }
            LogOpcode::ToL1Message => {
                vm_state.local_state.current_ergs_per_pubdata_byte
                    * zkvm_opcodes::system_params::L1_MESSAGE_PUBDATA_BYTES
            }
            _ => 0,
        };

        let extra_cost = match inner_variant {
            LogOpcode::PrecompileCall => low_u32_of_u256(&src1),
            _ => 0,
        };

        let total_cost = extra_cost + ergs_on_pubdata;

        let (ergs_remaining, not_enough_power) = ergs_available.overflowing_sub(total_cost);
        if not_enough_power {
            vm_state
                .local_state
                .callstack
                .get_current_stack_mut()
                .ergs_remaining = 0;

            vm_state.local_state.spent_pubdata_counter +=
                std::cmp::min(ergs_available, ergs_on_pubdata);
        } else {
            vm_state
                .local_state
                .callstack
                .get_current_stack_mut()
                .ergs_remaining = ergs_remaining;

            vm_state.local_state.spent_pubdata_counter += ergs_on_pubdata;
        }

        let current_context = vm_state.local_state.callstack.get_current_stack();
        let address = current_context.this_address;
        let shard_id = current_context.this_shard_id;

        #[allow(dropping_references)]
        drop(current_context);

        match inner_variant {
            LogOpcode::StorageRead => {
                assert!(!not_enough_power);
                let key = src0;

                let partial_query = LogQuery {
                    timestamp: timestamp_for_log,
                    tx_number_in_block,
                    aux_byte: STORAGE_AUX_BYTE,
                    shard_id,
                    address,
                    key,
                    read_value: U256::ZERO,
                    written_value: U256::ZERO,
                    rw_flag: false,
                    rollback: false,
                    is_service: is_first_message,
                };

                // we do not expect refunds for reads yet
                let query = vm_state
                    .access_storage(vm_state.local_state.monotonic_cycle_counter, partial_query);
                let result = PrimitiveValue { value: query.read_value, is_pointer: false };
                vm_state.perform_dst0_update(
                    vm_state.local_state.monotonic_cycle_counter,
                    result,
                    dst0_mem_location,
                    self,
                );
            }
            LogOpcode::StorageWrite => {
                if not_enough_power {
                    // we can return immediatelly and do not need to update regs
                    return;
                }
                let key = src0;
                let written_value = src1;

                let partial_query = LogQuery {
                    timestamp: timestamp_for_log,
                    tx_number_in_block,
                    aux_byte: STORAGE_AUX_BYTE,
                    shard_id,
                    address,
                    key,
                    read_value: U256::ZERO,
                    written_value,
                    rw_flag: true,
                    rollback: false,
                    is_service: is_first_message,
                };

                // we still do a formal query to execute write and record witness
                let _query = vm_state
                    .access_storage(vm_state.local_state.monotonic_cycle_counter, partial_query);
            }
            variant @ LogOpcode::Event | variant @ LogOpcode::ToL1Message => {
                if not_enough_power {
                    assert_eq!(variant, LogOpcode::ToL1Message);
                    // we do not add anything into log and do not need to update
                    // registers
                    return;
                }
                let key = src0;
                let written_value = src1;

                let aux_byte =
                    if variant == LogOpcode::Event { EVENT_AUX_BYTE } else { L1_MESSAGE_AUX_BYTE };

                let query = LogQuery {
                    timestamp: timestamp_for_log,
                    tx_number_in_block,
                    aux_byte,
                    shard_id,
                    address,
                    key,
                    read_value: U256::ZERO,
                    written_value,
                    rw_flag: true,
                    rollback: false,
                    is_service: is_first_message,
                };
                vm_state.emit_event(vm_state.local_state.monotonic_cycle_counter, query);
            }
            LogOpcode::PrecompileCall => {
                // add extra information about precompile abi in the "key" field

                if not_enough_power {
                    // we have to update register
                    vm_state.perform_dst0_update(
                        vm_state.local_state.monotonic_cycle_counter,
                        PrimitiveValue::empty(),
                        dst0_mem_location,
                        self,
                    );
                    return;
                }

                let mut precompile_abi = PrecompileCallABI::from_u256(src0);
                // normal execution
                vm_state
                    .local_state
                    .callstack
                    .get_current_stack_mut()
                    .ergs_remaining = ergs_remaining;
                if precompile_abi.memory_page_to_read == 0 {
                    let memory_page_to_read = CallStackEntry::<N, E>::heap_page_from_base(
                        vm_state
                            .local_state
                            .callstack
                            .get_current_stack()
                            .base_memory_page,
                    );
                    precompile_abi.memory_page_to_read = memory_page_to_read.0;
                }

                if precompile_abi.memory_page_to_write == 0 {
                    let memory_page_to_write = CallStackEntry::<N, E>::heap_page_from_base(
                        vm_state
                            .local_state
                            .callstack
                            .get_current_stack()
                            .base_memory_page,
                    );
                    precompile_abi.memory_page_to_write = memory_page_to_write.0;
                }

                let timestamp_to_read = vm_state.timestamp_for_first_decommit_or_precompile_read();
                debug_assert!(timestamp_to_read == timestamp_for_log);
                let timestamp_to_write =
                    vm_state.timestamp_for_second_decommit_or_precompile_write();
                debug_assert!(timestamp_to_read.0 + 1 == timestamp_to_write.0);

                let precompile_abi_encoded = precompile_abi.to_u256();

                let query = LogQuery {
                    timestamp: timestamp_for_log,
                    tx_number_in_block,
                    aux_byte: PRECOMPILE_AUX_BYTE,
                    shard_id,
                    address,
                    key: precompile_abi_encoded,
                    read_value: U256::ZERO,
                    written_value: U256::ZERO,
                    rw_flag: false,
                    rollback: false,
                    is_service: is_first_message,
                };

                vm_state.call_precompile(vm_state.local_state.monotonic_cycle_counter, query);
                let result = PrimitiveValue { value: U256::from(1u64), is_pointer: false };
                vm_state.perform_dst0_update(
                    vm_state.local_state.monotonic_cycle_counter,
                    result,
                    dst0_mem_location,
                    self,
                );
            }
        }
    }
}
