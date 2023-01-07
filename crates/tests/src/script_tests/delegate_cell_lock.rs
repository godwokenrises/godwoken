use ckb_types::{
    core::ScriptHashType,
    packed::{Bytes, CellDep, CellInput, CellOutput, Script, ScriptOpt, WitnessArgs},
    prelude::{Builder, Entity, Pack},
};

use super::{
    programs::{DELEGATE_CELL_LOCK_CODE_HASH, DELEGATE_CELL_LOCK_PROGRAM},
    utils::rollup::{random_always_success_script, CellContext},
};

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
#[repr(i8)]
enum Case {
    Success = 0,
    InvalidArgs = 5,
    CellDepNotFound,
    CellDepDataInvalid,
    InputNotFound,
    InvalidWitnessArgs,
}

#[test]
fn test_delegate_cell_lock() {
    for c in [
        Case::Success,
        Case::InvalidArgs,
        Case::CellDepNotFound,
        Case::CellDepDataInvalid,
        Case::InputNotFound,
        Case::InvalidWitnessArgs,
    ] {
        test_delegate_cell_lock_case(c);
    }
}

fn test_delegate_cell_lock_case(case: Case) {
    let mut ctx = CellContext::new(&Default::default(), Default::default());

    let always_success_lock = random_always_success_script();
    let delegate_cell_type_script = random_always_success_script();
    let delegate_cell_type_script_hash = delegate_cell_type_script.calc_script_hash();

    let delegate_cell = ctx.insert_cell(
        CellOutput::new_builder()
            .type_(
                ScriptOpt::new_builder()
                    .set(delegate_cell_type_script.into())
                    .build(),
            )
            .capacity(100.pack())
            .build(),
        if case == Case::CellDepDataInvalid {
            Default::default()
        } else {
            always_success_lock
                .calc_script_hash()
                .as_bytes()
                .slice(..20)
        },
    );

    let delegate_lock = ctx.insert_cell(Default::default(), DELEGATE_CELL_LOCK_PROGRAM.clone());

    let input = ctx.insert_cell(
        CellOutput::new_builder()
            .capacity(100.pack())
            .lock(
                Script::new_builder()
                    .code_hash(DELEGATE_CELL_LOCK_CODE_HASH.pack())
                    .hash_type(ScriptHashType::Data1.into())
                    .args(if case == Case::InvalidArgs {
                        Default::default()
                    } else {
                        delegate_cell_type_script_hash.as_bytes().pack()
                    })
                    .build(),
            )
            .build(),
        Default::default(),
    );
    let always_success_input = ctx.insert_cell(
        CellOutput::new_builder()
            .capacity(100.pack())
            .lock(always_success_lock)
            .build(),
        Default::default(),
    );

    let witness_args = {
        let mut w = WitnessArgs::new_builder();
        if case == Case::InvalidWitnessArgs {
            w = w.lock(Some(Bytes::default()).pack());
        }
        w.build()
    };
    let witness = witness_args.as_bytes().pack();

    let mut tx = ckb_types::packed::Transaction::default()
        .as_advanced_builder()
        .input(CellInput::new_builder().previous_output(input).build())
        .witness(witness)
        .cell_dep(
            CellDep::new_builder()
                .out_point(ctx.always_success_dep.out_point())
                .build(),
        )
        .cell_dep(CellDep::new_builder().out_point(delegate_lock).build());
    if case != Case::InputNotFound {
        tx = tx.input(
            CellInput::new_builder()
                .previous_output(always_success_input)
                .build(),
        );
    }
    if case != Case::CellDepNotFound {
        tx = tx.cell_dep(CellDep::new_builder().out_point(delegate_cell).build());
    }
    let tx = tx.build();
    match ctx.verify_tx(tx) {
        Ok(_) => assert_eq!(case, Case::Success),
        Err(e) => {
            let e = e.to_string();
            assert!(e.contains("Inputs[0].Lock"));
            assert!(e.contains(&format!("error code {} ", case as i8)));
        }
    }
}
