/******************************************************************
//
//
//  Copyright(C) 2020, Institute for Defense Analyses
//  4850 Mark Center Drive, Alexandria, VA; 703-845-2500
//
//  All rights reserved.
//  
//  This file is a part of Bale.  For license information see the
//  LICENSE file in the top level directory of the distribution.
//  
 *****************************************************************/ 
/*! \file spmat_agp.upc
 * \brief Sparse matrix support functions implemented with global addresses and atomics
 */
#include <spmat.h>
#include <sys/stat.h>   // for mkdir()
/******************************************************************************/
/*! \brief create a global int64_t array with a uniform random permutation
 * \param N the length of the global array
 * \param seed seed for the random number generator
 * \return the permutation
 * 
 * This is a collective call.
 * this implements the random dart algorithm to generate the permutation.
 * Each thread throws its elements of the perm array randomly at large target array.
 * Each element claims a unique entry in the large array using compare_and_swap.
 * This gives a random permutation with spaces in it, then you squeeze out the spaces.
 * \ingroup spmatgrp
 */
SHARED int64_t * rand_permp_agp(int64_t N, int seed) {  
  int64_t * ltarget, *lperm;
  int64_t r, i, j;
  int64_t pos, numdarts, numtargets, lnumtargets;

  lgp_rand_seed(seed);

  //T0_printf("Entering rand_permp_atomic...");fflush(0);

  SHARED int64_t * perm = lgp_all_alloc(N, sizeof(int64_t));
  if( perm == NULL ) return(NULL);
  lperm = lgp_local_part(int64_t, perm);

  int64_t l_N = (N + THREADS - MYTHREAD - 1)/THREADS;
  int64_t M = 2*N;
  int64_t l_M = (M + THREADS - MYTHREAD - 1)/THREADS;

  SHARED int64_t * target = lgp_all_alloc(M, sizeof(int64_t));
  if( target == NULL ) return(NULL);
  ltarget = lgp_local_part(int64_t, target);
  
  for(i=0; i<l_M; i++)
    ltarget[i] = -1L;
  lgp_barrier();

  i=0;
  while(i < l_N){                // throw the darts until you get l_N hits
    r = lgp_rand_int64(M);
    if( lgp_cmp_and_swap(target, r, -1L, (i*THREADS + MYTHREAD)) == (-1L) ){
      i++;
    }
  }
  lgp_barrier();

  numdarts = 0;
  for(i = 0; i < l_M; i++)    // count how many darts I got
    numdarts += (ltarget[i] != -1L );

  pos = lgp_prior_add_l(numdarts);    // my first index in the perm array is the number 
                                      // of elements produce by the smaller threads
  for(i = 0; i < l_M; i++){
    if(ltarget[i] != -1L ) {
       lgp_put_int64(perm, pos, ltarget[i]);
       pos++;
    }
  }

  lgp_all_free(target);
  lgp_barrier();
  //T0_printf("done!\n");
  return(perm);
}


/*! \brief apply row and column permutations to a sparse matrix using straight UPC
 * \param A pointer to the original matrix
 * \param rperm pointer to the global array holding the row permutation
 * \param cperm pointer to the global array holding the column permutation
 * rperm[i] = j means that row i of A goes to row j in matrix Ap
 * cperm[i] = j means that col i of A goes to col j in matrix Ap
 * \return a pointer to the matrix that has been produced or NULL if the model can't be used
 * \ingroup spmatgrp
 */
sparsemat_t * permute_matrix_agp(sparsemat_t *A, SHARED int64_t *rperm, SHARED int64_t *cperm) {
  //T0_printf("Permuting matrix with single puts\n");
  int weighted = (A->value != NULL);
  int64_t i, j, col, row, pos;
  int64_t * lrperm = lgp_local_part(int64_t, rperm);
  SHARED int64_t * rperminv = lgp_all_alloc(A->numrows, sizeof(int64_t));
  if( rperminv == NULL ) return(NULL);
  int64_t *lrperminv = lgp_local_part(int64_t, rperminv);

  //compute rperminv from rperm 
  for(i=0; i < A->lnumrows; i++){
    lgp_put_int64(rperminv, lrperm[i], i*THREADS + MYTHREAD);
  }

  lgp_barrier();
  
  int64_t cnt = 0, off, nxtoff;
  for(i = 0; i < A->lnumrows; i++){
    row = lrperminv[i];
    off    = lgp_get_int64(A->offset, row);
    nxtoff = lgp_get_int64(A->offset, row + THREADS);
    cnt += nxtoff - off;
  }
  lgp_barrier();

  sparsemat_t * Ap = init_matrix(A->numrows, A->numcols, cnt, weighted);
  
  // fill in permuted rows
  Ap->loffset[0] = pos = 0;
  for(i = 0; i < Ap->lnumrows; i++){
    row = lrperminv[i];
    off    = lgp_get_int64(A->offset, row);
    nxtoff = lgp_get_int64(A->offset, row + THREADS);
    lgp_memget(&Ap->lnonzero[pos], A->nonzero,
               (nxtoff-off)*sizeof(int64_t), off*THREADS + row%THREADS);    
    if(weighted)
      lgp_memget(&Ap->lvalue[pos], A->value,
                 (nxtoff-off)*sizeof(double), off*THREADS + row%THREADS);
    pos += nxtoff - off;
    //for(j = off; j < nxtoff; j++){
    //Ap->lnonzero[pos++] = lgp_get_int64(A->nonzero, j*THREADS + row%THREADS);
    //}
    Ap->loffset[i+1] = pos;
  }
  
  assert(pos == cnt);
  
  lgp_barrier();
  
  // finally permute column indices
  for(i = 0; i < Ap->lnumrows; i++){
    for(j = Ap->loffset[i]; j < Ap->loffset[i+1]; j++){
      Ap->lnonzero[j] = lgp_get_int64(cperm, Ap->lnonzero[j]);      
    }
  }
  lgp_barrier();

  lgp_all_free(rperminv);
  sort_nonzeros(Ap);

  return(Ap);
}

/*! \brief produce the transpose of a sparse matrix using UPC
 * \param A  pointer to the original matrix
 * \return a pointer to the matrix that has been produced or NULL if the model can't be used
 * \ingroup spmatgrp
 */
sparsemat_t * transpose_matrix_agp(sparsemat_t *A) {
  int64_t counted_nnz_At;
  int64_t lnnz, i, j, col, row, fromth, idx;
  int64_t pos;
  sparsemat_t * At;
  
  //T0_printf("UPC version of matrix transpose...");
  
  // find the number of nnz.s per thread

  SHARED int64_t * shtmp = lgp_all_alloc( A->numcols + THREADS, sizeof(int64_t));
  if( shtmp == NULL ) return(NULL);
  int64_t * l_shtmp = lgp_local_part(int64_t, shtmp);
  int64_t lnc = (A->numcols + THREADS - MYTHREAD - 1)/THREADS;
  for(i=0; i < lnc; i++)
    l_shtmp[i] = 0;
  lgp_barrier();

  for( i=0; i< A->lnnz; i++) {                   // histogram the column counts of A
    assert( A->lnonzero[i] < A->numcols );
    assert( A->lnonzero[i] >= 0 ); 
    pos = lgp_fetch_and_inc(shtmp, A->lnonzero[i]);
  }
  lgp_barrier();


  lnnz = 0;
  for( i = 0; i < lnc; i++) {
    lnnz += l_shtmp[i];
  }
  int weighted = (A->value != NULL);
  At = init_matrix(A->numcols, A->numrows, lnnz, weighted);
  if(!At){printf("ERROR: transpose_matrix_upc: init_matrix failed!\n");return(NULL);}

  int64_t sum = lgp_reduce_add_l(lnnz);      // check the histogram counted everything
  assert( A->nnz == sum ); 

  // compute the local offsets
  At->loffset[0] = 0;
  for(i = 1; i < At->lnumrows+1; i++)
    At->loffset[i] = At->loffset[i-1] + l_shtmp[i-1];

  // get the global indices of the start of each row of At
  for(i = 0; i < At->lnumrows; i++)
    l_shtmp[i] = MYTHREAD + THREADS * (At->loffset[i]);
    
  lgp_barrier();

  //redistribute the nonzeros 
  for(row=0; row<A->lnumrows; row++) {
    for(j=A->loffset[row]; j<A->loffset[row+1]; j++){
      pos = lgp_fetch_and_add(shtmp, A->lnonzero[j], (int64_t) THREADS);
      lgp_put_int64(At->nonzero, pos, row*THREADS + MYTHREAD);
      if(weighted) lgp_put_double(At->value, pos, A->lvalue[j]);
    }
  }

  lgp_barrier();
  //if(!MYTHREAD)printf("done\n");
  lgp_all_free(shtmp);

  return(At);
}

/*! \brief Write file called rowcnt_[thread number] into the given directory.
 * \param dirname The directory name
 * \param A  pointer to the matrix
 * \return 0 on success, -1 on failure
 * \ingroup spmatgrp
 */
int64_t write_rowcounts(char * dirname, sparsemat_t * A){
  int64_t i;
  
  SHARED int64_t * rowcnt = lgp_all_alloc(A->numrows, sizeof(int64_t));
  int64_t * lrowcnt = lgp_local_part(int64_t, rowcnt);

  for(i = 0; i < A->lnumrows; i++)
    lrowcnt[i] = A->loffset[i + 1] - A->loffset[i];
  lgp_barrier();

  char fname[64];
  sprintf(fname, "%s/rowcnt_%d", dirname, MYTHREAD);
  FILE * fp = fopen(fname, "wb");

  int64_t global_first_row = 0;
  for(i = 0; i < MYTHREAD; i++)
    global_first_row += (A->numrows + THREADS - i - 1)/THREADS;
  
  for(i = 0; i < A->lnumrows; i++){
    int64_t rc = lgp_get_int64(rowcnt, global_first_row + i);
    if( fwrite(&rc, sizeof(int64_t), 1,  fp) != 1 )
      return(-1);
  }
  fclose(fp);
  lgp_barrier();
  lgp_all_free(rowcnt);
  return(0);
}

/*! \brief Write a sparsemat_t to disk (in the directory dirname).
 * \param dirname The directory to write the sparse matrix data into.
 * \param A  pointer to the matrix
 * \return 0 on success, -1 on error
 * \ingroup spmatgrp
 */
int64_t write_sparse_matrix_agp(char * dirname, sparsemat_t * A){
  int64_t i;

  if(A->value){
    T0_fprintf(stderr,"WARNING: write_sparse_matrix_agp: writing a matrix with values not supported.\n");
    T0_fprintf(stderr,"Only, nonzero positions will be written.\n");
  }
  
  /* create the directory  */
  mkdir(dirname, 0770);
  
  /* write metadata ASCII file */
  write_sparse_matrix_metadata(dirname, A);
  
  /* write out the row counts */
  write_rowcounts(dirname, A);
  
  /* figure out max_row_cnt */
  int64_t global_first_row = 0;
  for(i = 0; i < MYTHREAD; i++)
    global_first_row += (A->numrows + THREADS - i - 1)/THREADS;

  int64_t max_row_cnt = 0;
  for(i = 0; i < A->lnumrows; i++)
    max_row_cnt = (max_row_cnt < (A->loffset[i+1] - A->loffset[i]) ? A->loffset[i+1] - A->loffset[i] : max_row_cnt);
  max_row_cnt = lgp_reduce_max_l(max_row_cnt);
  
  /* allocate write buffer */
  int64_t BUFSIZE = 128;
  while(BUFSIZE < max_row_cnt)
    BUFSIZE *= 2;
  int64_t * buf = calloc(BUFSIZE, sizeof(int64_t));

  char fname[64];
  sprintf(fname, "%s/nonzero_%d", dirname, MYTHREAD);
  FILE * fp = fopen(fname, "wb");
  
  /* write out the nonzeros in your block */
  int64_t pos = 0;
  for(i = global_first_row; i < global_first_row + A->lnumrows; i++){
    int64_t row_start = lgp_get_int64(A->offset, i);
    int64_t row_cnt = lgp_get_int64(A->offset, i + THREADS) - row_start;
    if(pos + row_cnt >= BUFSIZE){
      fwrite(buf, sizeof(int64_t), pos, fp);
      pos = 0;
    }
    lgp_memget(&buf[pos], A->nonzero, row_cnt*sizeof(int64_t), row_start*THREADS + i % THREADS);
    pos += row_cnt;
  }
  fwrite(buf, sizeof(int64_t), pos, fp);

  lgp_barrier();
  fclose(fp);
  
  free(buf);
  return(0);  
}

#if 0
sparsemat_t * read_sparse_matrix_agp(char * dirname){
  int64_t i;
  int64_t nr, nc, nnz, nwriters;
  int64_t lnr;
  
  /* read the ASCII file */
  read_sparse_matrix_metadata(dirname, &nr, &nc, &nnz, &nwriters);
  //fprintf(stderr,"%ld %ld %ld %ld\n", nc, nc, nnz, nwriters);

  /* read rowcnts */
  SHARED int64_t * rowcnt = read_rowcnts(dirname);
  
  /* calculate lnnz on each PE */
  lnr = (nr + THREADS - MYTHREAD - 1)/THREADS;
  for(i = 0; i < lnr; i++)
    PE_hist[i*THREADS + MYTHREAD] += lrowcnt[i];
  
  /* get offset array */
  
  return(NULL);
}
#endif
