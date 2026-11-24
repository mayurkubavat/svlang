typedef enum {READ, WRITE, IDLE} bus_kind_e;

// SVL-MNT-016 good: two classes only.
class bus_driver;
  int unsigned n;
  function void drive(bus_kind_e k);
    case (k)
      READ: n++;
      default: n--;
    endcase
  endfunction
endclass
class bus_monitor;
  int unsigned n;
  function void sample(bus_kind_e k);
    case (k)
      WRITE: n++;
      default: n--;
    endcase
  endfunction
endclass
