/******************************************************************
//
//
//  Copyright(C) 2020, Institute for Defense Analyses
//  4850 Mark Center Drive, Alexandria, VA; 703-845-2500
// 
//
//  All rights reserved.
//  
//   This file is a part of Bale.  For license information see the
//   LICENSE file in the top level directory of the distribution.
//  
// 
 *****************************************************************/ 

/*! \file sssp.h
 * \brief Demo application that implements Single Source Shortest Path algorithms.
 * 
 */

#ifndef sssp_INCLUDED
#define sssp_INCLUDED
#include <libgetput.h>
#include <exstack.h>
#include <convey.h>
#include <spmat.h>
#include <math.h>
#include <locale.h>

double        sssp_bellman_agp(d_array_t *tent, sparsemat_t * mat, int64_t v0);
double    sssp_bellman_exstack(d_array_t *tent, sparsemat_t * mat, int64_t buf_cnt, int64_t v0);
double   sssp_bellman_exstack2(d_array_t *tent, sparsemat_t * mat, int64_t buf_cnt, int64_t v0);
double     sssp_bellman_convey(d_array_t *tent, sparsemat_t * mat, int64_t v0);
double      sssp_delta_exstack(d_array_t *tent, sparsemat_t * mat, int64_t buf_cnt, int64_t v0, double opt_delta);
double     sssp_delta_exstack2(d_array_t *tent, sparsemat_t * mat, int64_t buf_cnt, int64_t v0, double opt_delta);
double       sssp_delta_convey(d_array_t *tent, sparsemat_t * mat, int64_t v0, double opt_delta);

void dump_tent(char *str, d_array_t *tent);

// The exstack and conveyor implementations all use the same package struct:

typedef struct sssp_pkg_t {
  //int64_t i; // We don't build the tree of paths from the vertices back along the shortest path to vertex 0.
               // If that were required, we would have to send the tail of the edge be relaxed.
               // This would not change any patterns, only increase the bandwidth demand.
  int64_t lj;  // the local "head" of the edge
  double tw;   // new tentative weight
}sssp_pkg_t ;

// alternates go here

#endif
