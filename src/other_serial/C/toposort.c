/*******************************************************************/
/* Copyright (c) 2020, Institute for Defense Analyses              */
/* 4850 Mark Center Drive, Alexandria, VA 22311-1882; 703-845-2500 */
/*                                                                 */
/* All rights reserved.                                            */
/*                                                                 */
/* This file is part of Bale.   For licence information see the    */
/* LICENSE file in the top level dirctory of the distribution.     */
/*******************************************************************/

/*! \file toposort.c
 * \brief Demo application that finds an upper triangular form for a matrix.  
 * That is, we are given a matrix that is a random row and column permutation 
 * of a an upper triangular matrix (with ones on the diagonal).
 * This algorithm finds a row and column permutation that would return it
 * to an upper triangular form.
 * 
 * Run topo --help or --usage for insructions on running.
 */

#include "spmat_utils.h"
#include "std_options.h"
#include "default_app_sizes.h"

/*  \page toposort_page toposort 
Find permutations that verify that a given matrix is morally upper triangular
*/ 

/* TODO move to README.md
   First we generate the problem by generating an upper triangular matrix
   and applying row and column permutations.
   
   The output of toposort is a row and a column permutation that, if applied,
   would result in an upper triangular matrix.

   We set the row and column permutations,  rperm and cperm, one pivot at a time.

   N = number of rows
   for( pos=N-1; pos > 0; pos-- ) {
     pick a row, r, with a single nonzero, c.
     say (r,c) is the pivot and set rperm[pos] = r and cprem[pos] = c
     Note: a pivot always exists because the matrix is morally upper tri.

     cross out that row r and col c 
   }

   Meaning of cross out:
   Rather than changing the matrix by deleting rows and column and then searching the 
   new matrix for the next pivot.  We do the obvious thing of keeping row counts, where
   rowcnt[i] is the number of non-zeros in row i and we use a really cool trick 
   of keeping the sum of the live column indices for the non-zeros in each row.
   That is, rowsum[i] is the sum of the column indices, not the sum of the non-zero elements,
   for the non-zeros in row i.  To "delete a column" one decrements the rowcnt by one and 
   the rowsum by the corrsponding column index. 
   The cool trick is that, when the rowcnt gets to one, the rowsum is the column that is left.
*/



/*! \brief check the result toposort 
 *
 * check that the permutations are in fact permutations and the check that applying
 * them to the original matrix yields an upper triangular matrix
 * \param mat the original matrix
 * \param rperminv the row permutation
 * \param cperminv the column permutation
 * \param dump_files debugging flag
 * \return 0 on success, 1 otherwise
 */
int check_result(sparsemat_t * mat, int64_t * rperminv, int64_t * cperminv, int64_t dump_files) 
{
  sparsemat_t * mat2;
  int ret = 0;
  
  int64_t rf = is_perm(rperminv, mat->numrows);
  int64_t cf = is_perm(cperminv, mat->numcols);
  if(!rf || !cf){
    fprintf(stderr,"ERROR: check_result is_perm(rperminv2) = %"PRId64" is_perm(cperminv2) = %"PRId64"\n",rf,cf);
    return(1);
  }
  mat2 = permute_matrix(mat, rperminv, cperminv);
  if(!is_upper_triangular(mat2, 1))
    ret = 1;
  if(dump_files) 
    dump_matrix(mat2, 20, "mat2.out");
  clear_matrix(mat2);
  free(mat2);
  return(ret);
}

/*! \brief Generate a matrix that is the a random permutation of a sparse uppper triangular matrix.
 * \param sargs the standard command line arguments
 * \param gargs the graph arguments line arguments
 * \return the permuted upper triangular matrix
 * 
 * Make the upper triangular matrix. We do this by getting the lower-triangular portion of the adjacency matrix of a random
 * graph. We force the diagonal entries. We then transpose this matrix to get U.
 * Finally, we randomly permute the rows and the columns.
 * The toposort algorithm takes this matrix and finds one of the possibly many row and column permutations 
 *  that would bring the matrix back to an upper triangular form.
 */
  // read in a matrix or generate a random graph
sparsemat_t *generate_toposort_input(std_args_t *sargs, std_graph_args_t *gargs)
{
  int64_t nr = gargs->numrows;
  printf("%ld \n",nr); 
  sparsemat_t * L = get_input_graph(sargs, gargs);
  if (!L) { fprintf(stderr, "ERROR: topo: L is NULL!\n");return(NULL); }
  sparsemat_t * U = transpose_matrix(L);
  if (!U) { fprintf(stderr, "ERROR: topo: U is NULL!\n");return(NULL); }
  if (!is_upper_triangular(U, 1)) {
    fprintf(stderr,"ERROR: generate_toposort did not start with an upper triangular\n");
    return(NULL);
  }
  clear_matrix(L); free(L);

  if(sargs->dump_files) write_matrix_mm(U, "topo_orig");
  
  // get random row and column permutations
  int64_t * rperminv = rand_perm(nr, 1234);
  int64_t * cperminv = rand_perm(nr, 5678);
  if(!rperminv || !cperminv){
    printf("ERROR: generate_toposort_input: rand_perm returned NULL!\n");
    exit(1);
  }
  //if(dump_files){
  //  dump_array(rperminv, numrows, 20, "rperm.out");
  //  dump_array(cperminv, numcols, 20, "cperm.out");
  //}
  
  sparsemat_t * mat = permute_matrix(U, rperminv, cperminv);
  if(!mat) {
    printf("ERROR: generate_toposort_input: permute_matrix returned NULL");
    return(NULL);
  }
  
  clear_matrix( U ); free( U );
  free(rperminv);
  free(cperminv);
  
  return(mat);
}


/*!
 * \brief This routine implements the agi variant of toposort
 * \param *rperm returns the row permutation that is found
 * \param *cperm returns the column permutation that is found
 * \param *mat the input sparse matrix NB. it must be a permuted upper triangular matrix 
 * \param *tmat the transpose of mat
 * \return average run time
 */
double toposort_matrix_queue(int64_t *rperm, int64_t *cperm, sparsemat_t *mat, sparsemat_t *tmat) 
{
  int64_t nr = mat->numrows;
  int64_t nc = mat->numcols;
  
  int64_t * queue  = calloc(nr, sizeof(int64_t));
  int64_t * rowtrck = calloc(nr, sizeof(int64_t));
  
  int64_t start, end;
  
  int64_t i, j, row, col, t_row;
   
  /* initialize rowsum, rowcnt, and queue (queue holds degree one rows) */
  start = end = 0;
  for(i = 0; i < mat->numrows; i++){
    rowtrck[i] = 0L;
    for(j = mat->offset[i]; j < mat->offset[i+1]; j++)
      rowtrck[i] += (1L<<32) + mat->nonzero[j];
    if((rowtrck[i] >> 32) ==  1)
      queue[end++] = i;    
  }
  
  // we a pick a row with a single nonzero = col.
  // setting rperm[pos] = row and cprem[pos] = col
  // in a sense 'moves' the row and col to the bottom
  // right corner of matrix.
  // Next, we cross out that row and col by decrementing 
  //  the rowcnt for any row that contains that col
  // repeat
  //
  double t1 = wall_seconds();
  
  int64_t n_pivots = 0;
  while(start < end){      
    row = queue[start++];
    col = rowtrck[row] & 0xFFFF;  // see cool trick
    
    rperm[row] = nr - 1 - n_pivots;
    cperm[col] = nc - 1 - n_pivots;
    n_pivots++;
  
    // look at this column (tmat's row) to find all the rows that hit it
    for(j=tmat->offset[col]; j < tmat->offset[col+1]; j++) {
      t_row = tmat->nonzero[j];
      assert((t_row) < mat->numrows);
      rowtrck[t_row] -= (1L<<32) + col;
      if( (rowtrck[t_row] >> 32) == 1L ) {
        queue[end++] = t_row;
      }
    }
  }
  
  t1 = wall_seconds() - t1;
  
  if(n_pivots != nr){
    printf("ERROR! toposort_matrix_queue: found %"PRId64" pivots but expected %"PRId64"!\n", n_pivots, nr);
    exit(1);
  }
  free(queue);
  free(rowtrck);
  return(t1);
}

/*!
 * \brief This routine implements the agi variant of toposort
 * \param *rperm returns the row permutation that is found
 * \param *cperm returns the column permutation that is found
 * \param *mat the input sparse matrix NB. it must be a permuted upper triangular matrix 
 * \param *tmat the transpose of mat
 * \return average run time
 */
double toposort_matrix_loop(int64_t *rperm, int64_t *cperm, sparsemat_t *mat, sparsemat_t *tmat) 
{
  int64_t nr = mat->numrows;
  int64_t nc = mat->numcols;
  
  int64_t * rowtrck = calloc(nr, sizeof(int64_t));
  
  int64_t i, j, col, t_row;
   
  /* initialize rowtrck */
  for(i = 0; i < nr; i++){
    rowtrck[i] = 0L;
    for(j = mat->offset[i]; j < mat->offset[i+1]; j++)
      rowtrck[i] += (1L<<32) + mat->nonzero[j];
  }
  
  // we a pick a row with a single nonzero = col.
  // setting rperm[pos] = row and cprem[pos] = col
  // in a sense 'moves' the row and col to the bottom
  // right corner of matrix.
  // Next, we cross out that row and col by decrementing 
  //  the rowcnt for any row that contains that col
  // repeat
  //
  double t1 = wall_seconds();
  
  int64_t n_pivots = 0;
  while(n_pivots < nr){      
    for(i = 0; i < nr; i++){
      if( (rowtrck[i] >> 32) == 1 ){
        col = rowtrck[i] & 0xFFFF;  // see cool trick
        rperm[i] = nr - 1 - n_pivots;
        cperm[col] = nc - 1 - n_pivots;
        n_pivots++;
        rowtrck[i] = 0L;
		
        // look at this column (tmat's row) to find all the rows that hit it
        for(j=tmat->offset[col]; j < tmat->offset[col+1]; j++) {
          t_row = tmat->nonzero[j];
          assert((t_row) < mat->numrows);
          rowtrck[t_row] -= (1L<<32) + col;
        }
      }
    }
  }
  
  t1 = wall_seconds() - t1;
  
  if(n_pivots != nr){
    printf("ERROR! toposort_matrix_queue: found %"PRId64" pivots but expected %"PRId64"!\n", n_pivots, nr);
    exit(1);
  }
  free(rowtrck);
  return(t1);
}


/********************************  argp setup  ************************************/
typedef struct args_t{
  int alg;
  std_args_t std;
  std_graph_args_t gstd;
}args_t;

static int parse_opt(int key, char * arg, struct argp_state * state)
{
  args_t * args = (args_t *)state->input;
  switch (key) {
  case 'a': args->alg = atoi(arg); break;     
  case ARGP_KEY_INIT:
    state->child_inputs[0] = &args->std;
    state->child_inputs[1] = &args->gstd;
    break;
  }
  return(0);
}

static struct argp_option options[] =
{
  {"toposort", 'a', "ALG", 0, "Algorithm: 0 means loops, 1 means queue"},  
  {0}
};

static struct argp_child children_parsers[] =
{
  {&std_options_argp, 0, "Standard Options", -2},
  {&std_graph_options_argp, 0, "Standard Graph Options", -3},
  {0}
};


int main(int argc, char * argv[])
{
  args_t args = {0};  
  struct argp argp = {options, parse_opt, 0, "Toposort", children_parsers};
  argp_parse(&argp, argc, argv, 0, 0, &args);
  args.gstd.numrows = 500;
  int ret = bale_app_init(argc, argv, &args, sizeof(args_t), &argp, &args.std);
  if (ret < 0) return(ret);
  else if (ret) return(0);
  
  //override command line 
  //(note:these will lead to matrices with not quite the right number of nonzeros 
  // if the user also used the -z flag.)
  if ( (args.gstd.loops == 0) || (args.gstd.directed == 1) ) {
    fprintf(stderr,"WARNING: toposort starts with a undirected graph with loops.\n");
    args.gstd.loops = 1;
    args.gstd.directed = 0;
  }

  write_std_graph_options(&args.std, &args.gstd);
  write_std_options(&args.std);
  
  sparsemat_t * mat = generate_toposort_input(&args.std, &args.gstd);
  if(!mat){printf("ERROR: topo: generate_toposort_input failed\n"); exit(1);}
  
  sparsemat_t * tmat = transpose_matrix(mat);
  if(!tmat){printf("ERROR: topo: transpose_matrix failed\n"); exit(1);}

  if (args.std.dump_files) {
    write_matrix_mm(mat, "topo_inmat");
    write_matrix_mm(tmat, "topo_tmat.mm");
    dump_matrix(mat,20, "mat.out");
    dump_matrix(tmat,20, "trans.out");
  }

  double laptime = 0.0;
  enum FLAVOR {GENERIC=1, LOOP=2, ALL=4};
  uint32_t use_model;
  // arrays to hold the row and col permutations
  int64_t *rperminv2 = calloc(mat->numrows, sizeof(int64_t));
  int64_t *cperminv2 = calloc(mat->numrows, sizeof(int64_t));
  int models_mask = (args.std.models_mask) ? args.std.models_mask : 3;
  for( use_model=1; use_model < ALL; use_model *=2 ) {
    switch( use_model & models_mask ) {
    case GENERIC:
      printf("   using generic toposort: ");                                 // TODO model_str thing
      laptime = toposort_matrix_queue(rperminv2, cperminv2, mat, tmat);
      break;
    case LOOP:
      printf("   using loop    toposort: ");                                 // TODO model_str thing
      laptime = toposort_matrix_loop(rperminv2, cperminv2, mat, tmat);
      break;
    }
    if( check_result(mat, rperminv2, cperminv2, args.std.dump_files) ) {
      fprintf(stderr,"\nERROR: After toposort_matrix_queue: mat2 is not upper-triangular!\n");
      exit(1);
    }
    printf("  %8.3lf seconds \n", laptime);
  }
  return(0);
}

