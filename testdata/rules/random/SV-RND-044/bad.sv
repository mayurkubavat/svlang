// Bad: 5 unique elements drawn from a 2-bit (4-value) domain
class bad_pigeonhole_unique;
  rand bit [1:0] ids[5];

  constraint uniq_ids_c {
    unique { ids };
  }
endclass
