use CyclicDist, BlockDist;
use Random;
use Assert;
use Time;
config const M = 20; //table size
config const N = 10; //num reads

// allocate main table (T), dest array (tgt) and array of random ints (rindex).
const Nspace = {0..N*numLocales-1};
const Mspace = {0..M-1};

const D = Mspace dmapped Cyclic(startIdx=Mspace.low);
var T: [D] int;

const D2 = Nspace dmapped Block(Nspace);
var rindex: [D2] int;
var tgt: [D2] int;
var t: Timer;
t.start();

/* set up loop: 
   populate the rindex array with random numbers mod M 
   populate T so that T[i] = i.
*/
fillRandom(rindex, 208); //208 is just the seed
rindex = mod(rindex, M);

T = {0..#M}; // identity permutation

t.stop();
writeln("Set up time: ", t.elapsed());
t.start();

/* Main loop 1 */
forall i in rindex.domain{
  tgt[i] = T[rindex[i]];
}

t.stop();
writeln("Loop 1: ", t.elapsed());
t.start();

/* Main loop 2 */
/* We could switch the main loop to this… */
coforall loc in Locales do on loc do{
    var inds = D2.localSubdomain();
    forall i in inds{
      tgt[i] += T[rindex[i]];
    }
}

t.stop();
writeln("Loop 2: ", t.elapsed());
t.start();

//We can check for success easily:
[r in rindex] r = 2*r;
if(!tgt.equals(rindex)){
  writeln("Error!");
}
