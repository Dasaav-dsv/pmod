#pragma once

#ifndef PMOD_PMOD_H
#define PMOD_PMOD_H

#ifdef _MSC_VER
#define PMOD_DLL extern __declspec(dllimport)
#else
#define PMOD_DLL extern
#endif

#ifdef __cplusplus
#include <cstdint>
extern "C" {
#else
#include <stdint.h>
#include <wchar.h>
#endif

/**
 * Get a pointer to the field data of a param row from the table `table_name`.
 * 
 * `table_name` must not be null and `id` must not be negative.
 * 
 * If the function fails it returns `NULL`.
 * 
 */
PMOD_DLL void* pmod_get_row(const char* table_name, int32_t id);

/**
 * Create a new param row in the table `table_name` and get its id.
 * 
 * `table_name` and `data` must not be null, and `data` must be valid for the
 * duration of the program.
 * 
 * If the function fails it returns a negative value.
 * 
 */
PMOD_DLL int32_t pmod_insert_row(const char* table_name, void* data);

/**
 * Replace the field data pointer of a param row and get a pointer
 * to its old data from the table `table_name`.
 * 
 * `table_name` and `data` must not be null and `id` must not be negative,
 * and `data` must be valid for the duration of the program.
 * 
 * If the function fails it returns `NULL`.
 * 
 */
PMOD_DLL void* pmod_replace_row(const char* table_name, int32_t id, void* data);

/**
 * Delete a param row and get a pointer to its field data from the table `table_name`.
 * 
 * `table_name` must not be null and `id` must not be negative.
 * 
 * If the function fails it returns `NULL`.
 * 
 */
PMOD_DLL void* pmod_delete_row(const char* table_name, int32_t id);

/**
 * Get a wide null terminated string from the message repository.
 * 
 * If the function fails it returns `NULL`.
 * 
 */
PMOD_DLL wchar_t* pmod_get_msg(uint32_t version, uint32_t category, uint32_t id);

/**
 * Insert a new a wide null terminated string in the message repository
 * and get its non-zero id.
 * 
 * `data` must not be null.
 * 
 * If the function fails it returns zero.
 * 
 */
PMOD_DLL uint32_t pmod_insert_msg(uint32_t version, uint32_t category, wchar_t* data);

/**
 * Replace a wide null terminated string from the message repository with another
 * and get a pointer to the old one.
 * 
 * `data` must not be null.
 * 
 * If the function fails it returns `NULL`.
 * 
 */
PMOD_DLL wchar_t* pmod_replace_msg(uint32_t version, uint32_t category, uint32_t id, wchar_t* data);

/**
 * Delete a wide null terminated string from the message repository.
 * 
 * If the function fails it returns `NULL`.
 * 
 */
PMOD_DLL wchar_t* pmod_delete_msg(uint32_t version, uint32_t category, uint32_t id);

#ifdef __cplusplus
}
#endif

#endif
