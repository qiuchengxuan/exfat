use bitfield::bitfield;

bitfield! {
    #[derive(Copy, Clone, Debug, Default, PartialEq, PartialOrd)]
    pub struct Flags(u16);
    allocation_possible, set_allocation_possible: 0, 0;
    no_fat_chain, set_no_fat_chain: 1, 1;
}
